# Technical Specification: Proactive Quota Monitoring System

**Version**: 1.0.0
**Status**: Draft
**Last Updated**: 2026-01-10
**Author**: Engineering Team
**Related Epic**: [QUOTA-001](../epics/proactive-quota-monitoring-epic.md)
**Related PRD**: [Proactive Quota Monitoring PRD](../product-requirements/proactive-quota-monitoring-prd.md)

---

## Table of Contents

1. [System Overview](#system-overview)
2. [Architecture](#architecture)
3. [API Integration](#api-integration)
4. [Data Models](#data-models)
5. [Core Algorithms](#core-algorithms)
6. [Error Handling](#error-handling)
7. [Performance Considerations](#performance-considerations)
8. [Implementation Roadmap](#implementation-roadmap)

---

## System Overview

### Problem Analysis

**Current System (Reactive)**:
```
Request → TokenManager::get_token()
  ↓
Select account (round-robin or weighted)
  ↓
Check RateLimitTracker (only post-429 data)
  ↓
Make API request
  ↓
429 Error → mark_rate_limited() → Switch account → Retry
  ↓
Result: 15-20% 429 error rate, 2-5s retry latency
```

**Proposed System (Proactive)**:
```
Request → TokenManager::get_token()
  ↓
FOR each candidate account:
  ├─→ Check QuotaCache (remainingFraction)
  │     ├─→ If < 5%: Skip account
  │     └─→ If >= 5%: Continue
  ├─→ Check RateLimitTracker (existing logic)
  │     ├─→ If rate-limited: Skip
  │     └─→ If available: Continue
  └─→ Account passed all checks: Use it
  ↓
Make API request (higher success probability)
  ↓
Result: <3% 429 error rate, <500ms selection time
```

### Key Insights from Reverse Engineering

**Reference**: [quota-apis.md](../antigravity/api/quota-apis.md)

**Discovery 1: Real-Time Quota API**
- Google Antigravity exposes `fetchAvailableModels` API
- Returns `remainingFraction` (0.0-1.0) per model
- Returns `resetTime` (ISO 8601 UTC midnight)
- **Implication**: We can check quota **before** request fails

**Discovery 2: Per-Model Quota Tracking**
- Quotas are separate per model family (Gemini vs Claude)
- Claude Opus has independent quota from Gemini Flash
- **Implication**: Must track quota per (account, model) pair

**Discovery 3: Subscription Tier Impact** ([quota-apis.md#subscription-tiers](../antigravity/api/quota-apis.md#subscription-tiers))
- FREE tier: "Aggressive rate limiting (more 429 errors)"
- PRO tier: "Relaxed rate limiting (fewer 429 errors)"
- ULTRA tier: "Most relaxed rate limiting"
- **Implication**: Prioritizing PRO/ULTRA accounts reduces 429s

**Discovery 4: Two Types of 429** ([error-pattern-catalog.md#429-resource-exhausted](../antigravity/reference/error-pattern-catalog.md#429-resource-exhausted))
- **Type A**: Rate limit (temporary, `remainingFraction > 0`)
- **Type B**: Quota exhausted (daily limit, `remainingFraction = 0`)
- **Implication**: Different recovery strategies needed

---

## Architecture

### Component Diagram

```
┌──────────────────────────────────────────────────────────────┐
│              Antigravity Manager Proxy                       │
│                                                              │
│  ┌────────────────────────────────────────────────────┐     │
│  │          Request Handler Layer                     │     │
│  │  (claude.rs, openai.rs, gemini.rs, audio.rs)       │     │
│  └───────────────────┬────────────────────────────────┘     │
│                      │                                       │
│                      ▼                                       │
│  ┌────────────────────────────────────────────────────┐     │
│  │         TokenManager (Existing)                    │     │
│  │  ┌──────────────────────────────────────────────┐  │     │
│  │  │ get_token(quota_group, force_rotate,        │  │     │
│  │  │           session_id, model) ─────────┐     │  │     │
│  │  └───────────────────────────────────────┼─────┘  │     │
│  │                                          │         │     │
│  │  ┌───────────────────────────────────────▼──────┐ │     │
│  │  │ FOR each candidate account:               │ │     │
│  │  │   1. Check QuotaManager (NEW)  ◄──────────┼─┼─┐   │
│  │  │   2. Check RateLimitTracker (EXISTING)    │ │ │   │
│  │  │   3. Select if both pass                  │ │ │   │
│  │  └───────────────────────────────────────────┘ │ │   │
│  └────────────────────────────────────────────────┘ │   │
│                                                      │   │
│  ┌──────────────────────────────────────────────────▼───▼┐
│  │         QuotaManager (NEW MODULE)                     │
│  │                                                        │
│  │  ┌────────────────────────────────────────────────┐   │
│  │  │  QuotaCache (DashMap)                          │   │
│  │  │  Key: "account:model"                          │   │
│  │  │  Value: CachedQuotaInfo (fraction, reset, TTL)│   │
│  │  └────────────────────────────────────────────────┘   │
│  │                      ▲                                 │
│  │                      │                                 │
│  │  ┌───────────────────┴──────────────────────────┐     │
│  │  │  QuotaFetcher (API Client)                   │     │
│  │  │  - fetch_available_models()                  │     │
│  │  │  - load_code_assist()                        │     │
│  │  └────────────┬─────────────────────────────────┘     │
│  │               │                                        │
│  │  ┌────────────▼─────────────────────────────────┐     │
│  │  │  BackgroundMonitor (Tokio Task)              │     │
│  │  │  - Interval: 5 minutes                       │     │
│  │  │  - Refresh all account quotas                │     │
│  │  │  - Update cache atomically                   │     │
│  │  └──────────────────────────────────────────────┘     │
│  └────────────────────────────────────────────────────────┘
│                      ↓
│  ┌────────────────────────────────────────────────────────┐
│  │    Google Antigravity v1internal API                   │
│  │    - fetchAvailableModels (quota data)                 │
│  │    - loadCodeAssist (subscription tier)                │
│  └────────────────────────────────────────────────────────┘
└──────────────────────────────────────────────────────────────┘
```

---

## API Integration

### 1. fetchAvailableModels API

**Reference**: [quota-apis.md#fetch-available-models](../antigravity/api/quota-apis.md#1-fetch-available-models-with-quota-info)

**Purpose**: Fetch real-time quota information for all models

**Implementation**:
```rust
pub struct QuotaFetcher {
    client: reqwest::Client,
    primary_endpoint: String,
    fallback_endpoint: String,
}

impl QuotaFetcher {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
            primary_endpoint: "https://cloudcode-pa.googleapis.com/v1internal".to_string(),
            fallback_endpoint: "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal".to_string(),
        }
    }

    pub async fn fetch_available_models(
        &self,
        access_token: &str,
        project_id: &str
    ) -> Result<HashMap<String, QuotaInfo>, String> {
        let url = format!("{}:fetchAvailableModels", self.primary_endpoint);

        let response = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .header("User-Agent", "antigravity/1.13.3 darwin/arm64")
            .json(&serde_json::json!({
                "project": project_id
            }))
            .send()
            .await;

        match response {
            Ok(resp) => {
                let status = resp.status().as_u16();

                match status {
                    200 => {
                        let data: ModelsResponse = resp.json().await
                            .map_err(|e| format!("Parse error: {}", e))?;

                        Ok(self.parse_quota_info(data))
                    }
                    401 => {
                        tracing::warn!("[Quota-Fetch] 401 Unauthorized for account");
                        Err("Authentication failed - token may be expired".to_string())
                    }
                    403 => {
                        tracing::warn!("[Quota-Fetch] 403 Forbidden for account");
                        Err("Permission denied - account may be blocked".to_string())
                    }
                    404 => {
                        tracing::info!("[Quota-Fetch] 404 on primary, trying fallback endpoint");
                        self.fetch_from_fallback(access_token, project_id).await
                    }
                    429 => {
                        tracing::warn!("[Quota-Fetch] 429 Rate limited on quota fetch");
                        Err("Rate limited on quota API".to_string())
                    }
                    _ => {
                        let error_text = resp.text().await.unwrap_or_default();
                        Err(format!("API error {}: {}", status, error_text))
                    }
                }
            }
            Err(e) => {
                tracing::error!("[Quota-Fetch] Network error: {}", e);
                Err(format!("Network error: {}", e))
            }
        }
    }

    async fn fetch_from_fallback(
        &self,
        access_token: &str,
        project_id: &str
    ) -> Result<HashMap<String, QuotaInfo>, String> {
        let url = format!("{}:fetchAvailableModels", self.fallback_endpoint);

        tracing::info!("[Quota-Fetch] Trying fallback endpoint: {}", url);

        // Same logic as primary endpoint
        // ... (implementation similar to above)
    }

    fn parse_quota_info(&self, response: ModelsResponse) -> HashMap<String, QuotaInfo> {
        response.models
            .into_iter()
            .filter_map(|(model_name, model_info)| {
                model_info.quota_info.map(|qi| {
                    tracing::debug!(
                        "[Quota-Parse] Model {}: {}% remaining, resets {}",
                        model_name,
                        (qi.remaining_fraction * 100.0) as u8,
                        qi.reset_time
                    );
                    (model_name, qi)
                })
            })
            .collect()
    }
}
```

**Response Structures**:
```rust
#[derive(Debug, Deserialize)]
struct ModelsResponse {
    models: HashMap<String, ModelInfo>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    #[serde(rename = "displayName")]
    display_name: Option<String>,

    #[serde(rename = "supportedGenerationMethods")]
    supported_generation_methods: Option<Vec<String>>,

    #[serde(rename = "quotaInfo")]
    quota_info: Option<QuotaInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuotaInfo {
    #[serde(rename = "remainingFraction")]
    pub remaining_fraction: f64,

    #[serde(rename = "resetTime")]
    pub reset_time: String,  // ISO 8601: "2026-01-11T00:00:00Z"
}

impl QuotaInfo {
    pub fn to_system_time(&self) -> Result<SystemTime, String> {
        use chrono::DateTime;

        let dt = DateTime::parse_from_rfc3339(&self.reset_time)
            .map_err(|e| format!("Invalid resetTime '{}': {}", self.reset_time, e))?;

        Ok(SystemTime::UNIX_EPOCH + Duration::from_secs(dt.timestamp() as u64))
    }

    pub fn hours_until_reset(&self) -> Result<f64, String> {
        let reset_time = self.to_system_time()?;
        let now = SystemTime::now();

        let duration = reset_time.duration_since(now)
            .map_err(|_| "Reset time is in the past".to_string())?;

        Ok(duration.as_secs() as f64 / 3600.0)
    }
}
```

---

### 2. loadCodeAssist API (Subscription Tier Detection)

**Reference**: [quota-apis.md#load-code-assist](../antigravity/api/quota-apis.md#2-load-code-assist-project--subscription-discovery)

**Purpose**: Detect subscription tier (FREE/PRO/ULTRA) for account prioritization

**Implementation**:
```rust
impl QuotaFetcher {
    pub async fn load_code_assist(
        &self,
        access_token: &str
    ) -> Result<SubscriptionInfo, String> {
        let url = format!("{}:loadCodeAssist", self.primary_endpoint);

        let response = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "metadata": {
                    "ideType": "ANTIGRAVITY"
                }
            }))
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if response.status() != 200 {
            return Err(format!("API error: {}", response.status()));
        }

        let data: LoadCodeAssistResponse = response.json().await
            .map_err(|e| format!("Parse error: {}", e))?;

        Ok(SubscriptionInfo {
            tier: data.paid_tier
                .and_then(|t| t.id)
                .or_else(|| data.current_tier.and_then(|t| t.id))
                .unwrap_or_else(|| "FREE".to_string()),
            project_id: data.cloudaicompanion_project,
        })
    }
}

#[derive(Debug, Deserialize)]
struct LoadCodeAssistResponse {
    #[serde(rename = "cloudaicompanionProject")]
    cloudaicompanion_project: String,

    #[serde(rename = "currentTier")]
    current_tier: Option<TierInfo>,

    #[serde(rename = "paidTier")]
    paid_tier: Option<TierInfo>,
}

#[derive(Debug, Deserialize)]
struct TierInfo {
    id: Option<String>,  // "FREE", "PRO", "ULTRA"
}

#[derive(Debug, Clone)]
pub struct SubscriptionInfo {
    pub tier: String,
    pub project_id: String,
}
```

---

## Data Models

### QuotaManager

```rust
/// Main quota management system
pub struct QuotaManager {
    /// Quota cache: "account:model" -> CachedQuotaInfo
    cache: Arc<DashMap<String, CachedQuotaInfo>>,

    /// API client for fetching quotas
    fetcher: QuotaFetcher,

    /// Background monitoring task
    monitor_task: Option<tokio::task::JoinHandle<()>>,

    /// Configuration
    config: QuotaMonitoringConfig,
}

#[derive(Debug, Clone)]
pub struct QuotaMonitoringConfig {
    /// Enable/disable quota monitoring
    pub enabled: bool,

    /// Refresh interval in seconds (default: 300)
    pub refresh_interval_seconds: u64,

    /// Cache TTL in seconds (default: 300)
    pub cache_ttl_seconds: u64,

    /// Quota threshold for warnings (default: 0.10)
    pub warning_threshold: f64,

    /// Quota threshold for account switching (default: 0.05)
    pub critical_threshold: f64,
}

impl Default for QuotaMonitoringConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            refresh_interval_seconds: 300,  // 5 minutes
            cache_ttl_seconds: 300,
            warning_threshold: 0.10,
            critical_threshold: 0.05,
        }
    }
}
```

### CachedQuotaInfo

```rust
#[derive(Debug, Clone)]
pub struct CachedQuotaInfo {
    /// Remaining quota as fraction (0.0-1.0)
    pub remaining_fraction: f64,

    /// When quota resets (UTC midnight)
    pub reset_time: SystemTime,

    /// When this data was fetched
    pub fetched_at: SystemTime,

    /// Time-to-live for this cache entry
    pub ttl: Duration,
}

impl CachedQuotaInfo {
    pub fn is_fresh(&self) -> bool {
        let now = SystemTime::now();
        let age = now.duration_since(self.fetched_at).unwrap_or(Duration::MAX);
        age < self.ttl
    }

    pub fn is_exhausted(&self) -> bool {
        self.remaining_fraction < 0.01
    }

    pub fn is_critical(&self) -> bool {
        self.remaining_fraction < 0.05
    }

    pub fn is_warning(&self) -> bool {
        self.remaining_fraction < 0.10
    }

    pub fn health_status(&self) -> QuotaHealthStatus {
        if self.remaining_fraction >= 0.20 {
            QuotaHealthStatus::Healthy
        } else if self.remaining_fraction >= 0.10 {
            QuotaHealthStatus::Warning
        } else if self.remaining_fraction >= 0.05 {
            QuotaHealthStatus::Critical
        } else {
            QuotaHealthStatus::Exhausted
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum QuotaHealthStatus {
    Healthy,    // > 20%
    Warning,    // 10-20%
    Critical,   // 5-10%
    Exhausted,  // < 5%
}
```

---

## Core Algorithms

### Algorithm 1: Account Selection with Quota Awareness

**Reference**: [quota-apis.md#multi-account-strategy](../antigravity/api/quota-apis.md#2-multi-account-strategy)

```rust
pub async fn get_token_with_quota_check(
    &self,
    quota_group: &str,
    force_rotate: bool,
    session_id: Option<&str>,
    model: Option<&str>,
) -> Result<(String, String, String), String> {
    // Step 1: Get weighted candidates (existing logic)
    let mut weighted_candidates = self.get_weighted_candidates(quota_group, session_id).await;

    // Step 2: Re-score candidates with quota awareness
    if let Some(m) = model {
        self.apply_quota_scoring(&mut weighted_candidates, m).await;
    }

    // Step 3: Try candidates in priority order
    for candidate in weighted_candidates {
        // Check 1: Quota check (NEW)
        if let Some(m) = model {
            if let Some(quota_info) = self.quota_manager.get_quota(&candidate.account_id, m).await {
                // Critical threshold: Skip immediately
                if quota_info.remaining_fraction < self.quota_manager.config.critical_threshold {
                    tracing::warn!(
                        "⚠️ [Quota-Skip] {} model {} quota {:.1}% < {:.0}% threshold",
                        candidate.account_id,
                        m,
                        quota_info.remaining_fraction * 100.0,
                        self.quota_manager.config.critical_threshold * 100.0
                    );
                    continue;
                }

                // Warning threshold: Log but proceed
                if quota_info.remaining_fraction < self.quota_manager.config.warning_threshold {
                    tracing::warn!(
                        "⚠️ [Quota-Warning] {} model {} quota {:.1}%",
                        candidate.account_id,
                        m,
                        quota_info.remaining_fraction * 100.0
                    );
                }
            }
        }

        // Check 2: Rate limit check (EXISTING)
        let is_limited = if let Some(m) = model {
            self.rate_limit_tracker.is_rate_limited_for_model(&candidate.account_id, m)
        } else {
            self.is_rate_limited(&candidate.account_id)
        };

        if is_limited {
            tracing::debug!("[Rate-Limited] Skipping {} (rate limited)", candidate.account_id);
            continue;
        }

        // Account passed both checks
        return Ok((
            candidate.access_token.clone(),
            candidate.project_id.clone(),
            candidate.account_id.clone()
        ));
    }

    Err("All accounts unavailable (quota exhausted or rate limited)".to_string())
}

async fn apply_quota_scoring(&self, candidates: &mut [WeightedCandidate], model: &str) {
    for candidate in candidates.iter_mut() {
        if let Some(quota_info) = self.quota_manager.get_quota(&candidate.account_id, model).await {
            // Boost score for higher quota
            let quota_boost = quota_info.remaining_fraction * 50.0;
            candidate.weight += quota_boost;

            tracing::debug!(
                "[Quota-Score] {} quota {:.1}% → weight boost: +{:.1}",
                candidate.account_id,
                quota_info.remaining_fraction * 100.0,
                quota_boost
            );
        }
    }

    // Re-sort by updated weights
    candidates.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
}
```

---

### Algorithm 2: Subscription Tier Prioritization

**Reference**: [quota-apis.md#subscription-tiers](../antigravity/api/quota-apis.md#subscription-tiers)

```rust
fn calculate_tier_weight(tier: &str) -> f64 {
    match tier {
        "ULTRA" | "ws-ai-ultra-business-tier" => 300.0,
        "PRO" | "g1-pro-tier" => 200.0,
        _ => 100.0  // FREE or unknown
    }
}

fn calculate_account_score(
    account: &ProxyToken,
    quota_remaining: Option<f64>
) -> f64 {
    let tier_weight = calculate_tier_weight(&account.subscription_tier);
    let quota_weight = quota_remaining.unwrap_or(0.5) * 100.0;

    tier_weight + quota_weight
}

// Example scores:
// ULTRA + 80% quota = 300 + 80 = 380 ✅ Best
// PRO + 95% quota = 200 + 95 = 295
// FREE + 100% quota = 100 + 100 = 200
// PRO + 10% quota = 200 + 10 = 210
// FREE + 90% quota = 100 + 90 = 190 ✅ Worst
```

---

### Algorithm 3: Background Quota Refresh

```rust
pub async fn start_background_monitoring(&self) {
    let tokens = Arc::clone(&self.tokens);
    let quota_manager = Arc::clone(&self.quota_manager);
    let config = quota_manager.config.clone();

    if !config.enabled {
        tracing::info!("[Quota-Monitor] Background monitoring disabled in config");
        return;
    }

    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(
            Duration::from_secs(config.refresh_interval_seconds)
        );

        tracing::info!(
            "[Quota-Monitor] Started with {}s refresh interval",
            config.refresh_interval_seconds
        );

        loop {
            interval.tick().await;

            let start = std::time::Instant::now();
            let mut success_count = 0;
            let mut error_count = 0;

            tracing::info!("[Quota-Monitor] Starting refresh for {} accounts", tokens.len());

            for entry in tokens.iter() {
                let token = entry.value();

                match quota_manager.fetcher.fetch_available_models(
                    &token.access_token,
                    &token.project_id
                ).await {
                    Ok(quotas) => {
                        for (model, quota_info) in quotas {
                            quota_manager.update_cache(
                                &token.account_id,
                                &model,
                                quota_info,
                                config.cache_ttl_seconds
                            );
                        }
                        success_count += 1;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "[Quota-Monitor] Failed to refresh {}: {}",
                            token.account_id,
                            e
                        );
                        error_count += 1;
                    }
                }
            }

            let elapsed = start.elapsed();
            tracing::info!(
                "[Quota-Monitor] Refresh completed: {} success, {} errors, {:?} elapsed",
                success_count,
                error_count,
                elapsed
            );
        }
    });

    quota_manager.monitor_task = Some(task);
}
```

---

## Error Handling

### Error Categories

**Reference**: [error-pattern-catalog.md](../antigravity/reference/error-pattern-catalog.md)

#### 1. Authentication Errors (401)

**Scenario**: OAuth token expired during quota fetch

**Handling**:
```rust
if status == 401 {
    // Log but don't crash - account may need token refresh
    tracing::warn!("[Quota-Fetch] 401 for {}, token may be expired", account_id);

    // Mark for token refresh on next request
    // (don't refresh here to avoid blocking background task)

    return Err("Authentication failed".to_string());
}
```

#### 2. Permission Errors (403)

**Scenario**: Account blocked or API not enabled

**Handling**:
```rust
if status == 403 {
    // Permanent error - mark account
    tracing::error!("[Quota-Fetch] 403 for {}, account may be blocked", account_id);

    // Mark account as forbidden (don't retry quota fetch)
    account.is_forbidden = true;

    return Err("Permission denied".to_string());
}
```

#### 3. Rate Limiting (429) on Quota API

**Scenario**: Too many quota fetch requests

**Handling**:
```rust
if status == 429 {
    // Quota API itself is rate limited
    tracing::warn!("[Quota-Fetch] 429 on quota API, skipping this refresh cycle");

    // Don't retry immediately - wait for next interval
    // Use cached data if available

    return Err("Rate limited on quota fetch".to_string());
}
```

#### 4. Endpoint Unavailable (404, 500, 503)

**Handling** ([error-pattern-catalog.md#recovery-strategies](../antigravity/reference/error-pattern-catalog.md#recovery-strategies)):
```rust
match status {
    404 => {
        // Try fallback endpoint
        return self.fetch_from_fallback(access_token, project_id).await;
    }
    500 | 503 => {
        // Temporary error - retry next cycle
        tracing::warn!("[Quota-Fetch] {}error, will retry next cycle", status);
        return Err(format!("Server error: {}", status));
    }
    _ => {
        return Err(format!("Unexpected error: {}", status));
    }
}
```

---

## Performance Considerations

### Caching Strategy

**Cache Hit Optimization**:
```yaml
cache_design:
  key_format: "account_id:model"
  storage: DashMap (lock-free concurrent HashMap)
  ttl: 300 seconds (5 minutes)

  cache_lifecycle:
    - INSERT: On successful quota fetch
    - UPDATE: On background refresh
    - INVALIDATE: On 429 error (quota exhausted)
    - EXPIRE: After TTL elapsed

  expected_hit_rate: "> 80% (after warmup)"
```

**Cache Warming**:
```rust
pub async fn warm_cache(&self) -> Result<(), String> {
    tracing::info!("[Quota-Cache] Warming cache for all accounts");

    for entry in self.tokens.iter() {
        let token = entry.value();

        if let Ok(quotas) = self.quota_manager.fetcher.fetch_available_models(
            &token.access_token,
            &token.project_id
        ).await {
            for (model, quota_info) in quotas {
                self.quota_manager.update_cache(
                    &token.account_id,
                    &model,
                    quota_info,
                    self.quota_manager.config.cache_ttl_seconds
                );
            }
        }
    }

    tracing::info!("[Quota-Cache] Cache warming completed");
    Ok(())
}
```

---

### Latency Budget

**Performance Targets**:
```yaml
quota_check_latency:
  cache_hit: "< 1ms (DashMap lookup)"
  cache_miss_api: "< 200ms (API call + parse)"
  background_refresh: "< 5s (all accounts)"

request_flow_impact:
  without_quota_check: "baseline"
  with_quota_check_cached: "+1ms (negligible)"
  with_quota_check_miss: "+200ms (acceptable)"

  target: "< 5% latency increase on average"
```

---

### Memory Footprint

**Estimation**:
```yaml
data_structures:
  CachedQuotaInfo: ~120 bytes per entry
  DashMap overhead: ~40 bytes per entry

  per_account:
    models: ~25 models per account
    entries: 25 * (120 + 40) = 4 KB

  total_11_accounts:
    memory: 11 * 4 KB = 44 KB

  overhead:
    - DashMap structure: ~5 KB
    - QuotaManager: ~2 KB

  total_footprint: ~50 KB (negligible)
```

---

## Implementation Roadmap

### Phase 1: API Integration (Week 1)

**Components**:
- [ ] `quota_manager.rs` module creation
- [ ] `QuotaFetcher` implementation
- [ ] API response parsing
- [ ] Unit tests

**Deliverables**:
- Working `fetch_available_models()` function
- Parsed quota data structures
- >80% test coverage

---

### Phase 2: Cache & Pre-Request Check (Week 2)

**Components**:
- [ ] `QuotaCache` implementation
- [ ] Pre-request validation in `get_token()`
- [ ] Threshold-based decision logic
- [ ] Integration with TokenManager

**Deliverables**:
- Quota cache operational
- Pre-request checks working
- Account switching on low quota

---

### Phase 3: Background Monitoring (Week 3)

**Components**:
- [ ] Background Tokio task
- [ ] Subscription tier detection (`loadCodeAssist`)
- [ ] Tier-based prioritization
- [ ] Configuration loading

**Deliverables**:
- Background refresh working
- Tier prioritization active
- 48-hour stability test passed

---

### Phase 4: UI & Polish (Week 4)

**Components**:
- [ ] Quota health UI indicators
- [ ] Tauri event emission
- [ ] Documentation updates
- [ ] Performance optimization

**Deliverables**:
- UI quota display
- Complete documentation
- Production-ready code

---

## Testing Strategy

### Unit Tests

**Coverage Target**: >80%

**Key Test Cases**:
- Quota API parsing (success, errors)
- Cache operations (get, set, expiration)
- Pre-request validation logic
- Tier-based scoring algorithm
- Background task behavior

---

### Integration Tests

**Scenarios**:
- End-to-end quota fetch → cache → pre-request check
- Background monitoring with 11 accounts
- Account switching on quota exhaustion
- Fallback to Gemini when all Claude quotas exhausted

---

### Performance Tests

**Benchmarks**:
- Cache lookup latency: <1ms
- API call latency: <200ms
- Background refresh: <5s for 11 accounts
- Memory stability (no leaks over 48h)

---

### Load Tests

**Scenarios**:
- 100 concurrent requests with quota checks
- Quota cache hit rate measurement
- Performance impact assessment

---

## Monitoring & Observability

### Logging

**Log Levels**:
```yaml
INFO:
  - Background monitoring start/completion
  - Quota refresh successes
  - Account selection with quota score

WARN:
  - Low quota warnings (<10%)
  - Quota fetch failures
  - Account skipped due to critical quota

DEBUG:
  - Cache hits/misses
  - Quota scoring calculations
  - Pre-request check results

ERROR:
  - API failures (persistent)
  - Configuration errors
  - Critical system issues
```

**Log Examples**:
```
[INFO] [Quota-Monitor] Starting refresh for 11 accounts
[INFO] ✅ [Quota-OK] acc@gmail.com model claude-opus quota 85.0%
[WARN] ⚠️ [Quota-Warning] acc@gmail.com model claude-opus quota 7.5%
[WARN] ⚠️ [Quota-Skip] acc@gmail.com model claude-opus quota 2.1% < 5.0% threshold
[INFO] [Quota-Monitor] Refresh completed: 10 success, 1 errors, 3.2s elapsed
```

---

### Metrics

**Prometheus-Style Metrics** (future):
```yaml
quota_remaining_fraction:
  type: gauge
  labels: [account_id, model]
  description: "Remaining quota as fraction (0.0-1.0)"

quota_fetch_duration_seconds:
  type: histogram
  description: "Duration of quota API calls"

quota_cache_hit_ratio:
  type: gauge
  description: "Percentage of cache hits vs misses"

account_switch_total:
  type: counter
  labels: [reason]
  description: "Total account switches (quota vs rate_limit)"

http_429_errors_total:
  type: counter
  labels: [model]
  description: "Total 429 errors (target: <3%)"
```

---

## Configuration

### Default Configuration

**File**: `src-tauri/src/proxy/config.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    // ... existing fields ...

    #[serde(default)]
    pub quota_monitoring: QuotaMonitoringConfig,
}

impl Default for QuotaMonitoringConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            refresh_interval_seconds: 300,  // 5 minutes
            cache_ttl_seconds: 300,         // 5 minutes
            warning_threshold: 0.10,         // 10%
            critical_threshold: 0.05,        // 5%
        }
    }
}
```

### User Configuration

**File**: `{data_dir}/config.json`

```json
{
  "proxy_config": {
    "quota_monitoring": {
      "enabled": true,
      "refresh_interval_seconds": 300,
      "cache_ttl_seconds": 300,
      "warning_threshold": 0.10,
      "critical_threshold": 0.05
    }
  }
}
```

---

## Rollout Strategy

### Feature Flag

```rust
if !config.quota_monitoring.enabled {
    // Fallback to reactive mode (existing behavior)
    tracing::info!("[Quota] Proactive monitoring disabled, using reactive mode");
    return self.get_token_reactive(quota_group, force_rotate, session_id, model).await;
}
```

### Gradual Rollout

**Phase 1**: Internal testing (engineering team)
**Phase 2**: Beta testing (select users, feature flag enabled)
**Phase 3**: General availability (all users, feature flag default ON)

---

## Success Validation

### Pre-Launch Checklist

- [ ] All unit tests passing (>80% coverage)
- [ ] Integration tests passing
- [ ] 48-hour stability test completed
- [ ] Performance benchmarks met
- [ ] Code review approved
- [ ] Documentation complete

### Post-Launch Metrics (7 days)

- [ ] 429 error rate: <5% (improvement from 15-20%)
- [ ] Cache hit rate: >80%
- [ ] Background task uptime: >99%
- [ ] No critical bugs reported

### Post-Launch Metrics (30 days)

- [ ] 429 error rate: <3% (final target)
- [ ] Account switch latency: <500ms
- [ ] API success rate: >95%
- [ ] User satisfaction: Positive feedback

---

## References

### Google Antigravity Reverse Engineering

**Core Documentation**:
- [Quota Management APIs](../antigravity/api/quota-apis.md) - Complete API reference
- [Error Pattern Catalog](../antigravity/reference/error-pattern-catalog.md) - 429 error handling
- [Subscription Tiers](../antigravity/api/quota-apis.md#subscription-tiers) - Tier system
- [Best Practices](../antigravity/api/quota-apis.md#best-practices) - Quota optimization

**Architecture**:
- [v1internal API Specification](../antigravity/architecture/v1internal-api.md)
- [Request/Response Flow](../antigravity/api/request-response-flow.md)

**Examples**:
- [Complete Examples](../antigravity/examples/complete-examples.md) - Working requests/responses

---

### Related Documents

**Product Management**:
- [PRD: Proactive Quota Monitoring](../product-requirements/proactive-quota-monitoring-prd.md)
- [Epic QUOTA-001](../epics/proactive-quota-monitoring-epic.md)

**User Stories**:
- [QUOTA-001-01: API Integration](../stories/QUOTA-001-01-quota-api-integration.md)
- [QUOTA-001-02: Pre-Request Validation](../stories/QUOTA-001-02-pre-request-validation.md)
- [QUOTA-001-03: Background Monitoring](../stories/QUOTA-001-03-background-monitoring.md)
- [QUOTA-001-04: Subscription Tier](../stories/QUOTA-001-04-subscription-tier.md)
- [QUOTA-001-05: Quota Cache](../stories/QUOTA-001-05-quota-cache.md)
- [QUOTA-001-06: UI Indicators](../stories/QUOTA-001-06-ui-indicators.md)

---

## Appendix

### A. Code Structure

**New Files**:
```
src-tauri/src/proxy/
├── quota_manager.rs       (NEW) - Main quota management
├── quota_cache.rs         (NEW) - Cache implementation
├── quota_fetcher.rs       (NEW) - API client
└── token_manager.rs       (MODIFIED) - Integration point
```

### B. Configuration Schema

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "type": "object",
  "properties": {
    "quota_monitoring": {
      "type": "object",
      "properties": {
        "enabled": {"type": "boolean", "default": true},
        "refresh_interval_seconds": {"type": "integer", "minimum": 60, "maximum": 3600, "default": 300},
        "cache_ttl_seconds": {"type": "integer", "minimum": 60, "maximum": 3600, "default": 300},
        "warning_threshold": {"type": "number", "minimum": 0.0, "maximum": 1.0, "default": 0.10},
        "critical_threshold": {"type": "number", "minimum": 0.0, "maximum": 1.0, "default": 0.05}
      }
    }
  }
}
```

---

**Document History**:
- 2026-01-10: Initial technical specification (v1.0.0)
