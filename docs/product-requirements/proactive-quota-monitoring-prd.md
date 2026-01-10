# Product Requirements Document: Proactive Quota Monitoring System

**Document Status**: Draft
**Version**: 1.0.0
**Last Updated**: 2026-01-10
**Author**: Product Team
**Stakeholders**: Engineering, DevOps, Users

---

## Executive Summary

### Problem Statement

Users experience frequent **429 (RESOURCE_EXHAUSTED)** errors due to reactive quota management. The current system only detects quota exhaustion **after** a failed request, resulting in:

- **Poor User Experience**: Interrupted workflows, failed requests
- **Resource Waste**: Unnecessary API calls to exhausted accounts
- **Inefficient Account Rotation**: Delayed switching to available accounts
- **Limited Visibility**: Users unaware of approaching quota limits

### Proposed Solution

Implement **proactive quota monitoring** using Google Antigravity's `fetchAvailableModels` API to:

1. **Check quota before requests** - Prevent 429 errors before they occur
2. **Monitor quota in real-time** - Track `remainingFraction` across all accounts
3. **Intelligent account selection** - Prioritize accounts with higher available quota
4. **Predictive warnings** - Alert users when quota drops below thresholds
5. **Subscription tier awareness** - Leverage PRO/ULTRA tier higher limits

### Success Metrics

| Metric | Current | Target | Measurement |
|--------|---------|--------|-------------|
| **429 Error Rate** | ~15-20% of requests | <3% of requests | Error logs |
| **Quota Exhaustion Detection** | Reactive (post-failure) | Proactive (pre-request) | Request success rate |
| **Account Switch Latency** | 2-5 seconds | <500ms | Performance metrics |
| **User-Visible Errors** | High | Low | User feedback |
| **API Call Efficiency** | 70-80% success | >95% success | API metrics |

---

## Background & Research

### Reverse Engineering Findings

**Source Documentation**:
- [Quota Management APIs](../antigravity/api/quota-apis.md)
- [Error Pattern Catalog](../antigravity/reference/error-pattern-catalog.md)
- [Complete Examples](../antigravity/examples/complete-examples.md)

#### Key Discoveries

**1. Google Antigravity Quota Architecture** ([quota-apis.md](../antigravity/api/quota-apis.md)):

```yaml
quota_system:
  api_endpoint: "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels"

  quota_tracking:
    granularity: "Per model family (gemini vs claude)"
    unit: "remainingFraction (0.0-1.0)"
    reset_frequency: "Daily at UTC midnight"

  subscription_tiers:
    FREE:
      quota_limits: "Limited requests per day"
      rate_limiting: "Aggressive (more 429 errors)"
    PRO:
      quota_limits: "Higher request limit"
      rate_limiting: "Relaxed (fewer 429 errors)"
    ULTRA:
      quota_limits: "Highest request limit"
      rate_limiting: "Most relaxed"
```

**2. Quota Info Data Structure**:

```protobuf
message QuotaInfo {
  double remainingFraction = 1;  // 0.0-1.0 (0% to 100%)
  string resetTime = 2;           // ISO 8601: "2026-01-11T00:00:00Z"
}
```

**3. Two Types of 429 Errors** ([error-pattern-catalog.md](../antigravity/reference/error-pattern-catalog.md)):

**Type A: Rate Limit (Temporary)**
```json
{
  "error": {
    "code": 429,
    "status": "RESOURCE_EXHAUSTED"
  }
}
```
- **Condition**: `remainingFraction > 0` (quota still available)
- **Cause**: Too many requests per second
- **Recovery**: Wait 5 seconds, retry

**Type B: Quota Exhausted (Daily Limit)**
```json
{
  "error": {
    "code": 429,
    "message": "You have exhausted your capacity on this model...",
    "details": [{
      "metadata": {
        "quotaResetDelay": "81h1m19s",
        "quotaResetTimeStamp": "2026-01-13T06:26:53Z",
        "model": "claude-opus-4-5-thinking"
      }
    }]
  }
}
```
- **Condition**: `remainingFraction === 0` (quota fully exhausted)
- **Cause**: Daily quota limit reached
- **Recovery**: Switch account OR wait until `resetTime` (UTC midnight)

---

## Requirements

### Functional Requirements

#### FR-1: Quota Fetching API Integration

**Priority**: P0 (Critical)
**Reference**: [quota-apis.md#fetch-available-models](../antigravity/api/quota-apis.md#1-fetch-available-models-with-quota-info)

**Description**: Implement `fetchAvailableModels` API integration to retrieve real-time quota information.

**Acceptance Criteria**:
- [ ] API client for `POST /v1internal:fetchAvailableModels`
- [ ] Parse `remainingFraction` and `resetTime` per model
- [ ] Handle authentication (Bearer token)
- [ ] Error handling for API failures
- [ ] Response caching (5-minute TTL)

**API Specification**:
```http
POST https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels
Authorization: Bearer {access_token}
Content-Type: application/json

{
  "project": "bamboo-precept-lgxtn"
}
```

**Response Structure**:
```json
{
  "models": {
    "claude-opus-4-5-thinking": {
      "quotaInfo": {
        "remainingFraction": 0.87,
        "resetTime": "2026-01-11T00:00:00Z"
      }
    },
    "gemini-2.5-flash": {
      "quotaInfo": {
        "remainingFraction": 0.95,
        "resetTime": "2026-01-11T00:00:00Z"
      }
    }
  }
}
```

---

#### FR-2: Pre-Request Quota Validation

**Priority**: P0 (Critical)
**Reference**: [quota-apis.md#monitoring-strategies](../antigravity/api/quota-apis.md#monitoring-strategies)

**Description**: Check quota availability before making API requests to prevent 429 errors.

**Acceptance Criteria**:
- [ ] Pre-request quota check function
- [ ] Threshold-based warnings (10%, 5%, 0%)
- [ ] Account switch recommendation when quota low
- [ ] Logging for quota checks

**Quota Thresholds**:
```yaml
quota_health:
  healthy: "> 20% (0.20)"
  warning: "10-20% (0.10-0.20)"
  critical: "< 10% (0.10)"
  exhausted: "0% (0.0)"
```

**Logic Flow**:
```
Before API Request:
  ↓
Check remainingFraction for (account, model):
  ↓
If remainingFraction > 0.10:
  → Proceed with request
  ↓
If remainingFraction 0.05-0.10:
  → Log warning
  → Proceed with request
  ↓
If remainingFraction 0.0-0.05:
  → Switch to next account
  → Retry with new account
  ↓
If ALL accounts < 0.05:
  → Fallback to alternative model (Gemini)
  → OR queue request until reset
```

---

#### FR-3: Background Quota Monitoring

**Priority**: P1 (High)
**Reference**: [quota-apis.md#aggregate-tracking](../antigravity/api/quota-apis.md#aggregate-tracking)

**Description**: Periodically refresh quota information for all accounts without blocking request flow.

**Acceptance Criteria**:
- [ ] Background task every 5 minutes
- [ ] Refresh quotas for all accounts
- [ ] Update quota cache atomically
- [ ] Handle refresh failures gracefully
- [ ] Configurable refresh interval

**Implementation Pattern**:
```rust
pub async fn start_quota_monitoring(&self) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 min

        loop {
            interval.tick().await;

            for account in self.tokens.iter() {
                match fetch_available_models(&account.access_token, &account.project_id).await {
                    Ok(quotas) => {
                        self.update_quota_cache(&account.account_id, quotas);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to refresh quota for {}: {}", account.account_id, e);
                    }
                }
            }
        }
    });
}
```

---

#### FR-4: Subscription Tier Awareness

**Priority**: P1 (High)
**Reference**: [quota-apis.md#subscription-tiers](../antigravity/api/quota-apis.md#subscription-tiers)

**Description**: Prioritize account selection based on subscription tier (ULTRA > PRO > FREE) to maximize quota availability.

**Acceptance Criteria**:
- [ ] Detect subscription tier via `loadCodeAssist` API
- [ ] Store tier information per account
- [ ] Prioritize higher-tier accounts in selection algorithm
- [ ] Display tier in account list UI

**Tier Detection** ([quota-apis.md#load-code-assist](../antigravity/api/quota-apis.md#2-load-code-assist-project--subscription-discovery)):
```http
POST https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist

{
  "metadata": {
    "ideType": "ANTIGRAVITY"
  }
}
```

**Response**:
```json
{
  "currentTier": {
    "id": "PRO",
    "quotaTier": "pro"
  },
  "paidTier": {
    "id": "ULTRA",
    "quotaTier": "ultra"
  }
}
```

**Priority Logic**:
```rust
fn prioritize_accounts(&self) -> Vec<String> {
    let mut accounts: Vec<_> = self.tokens.iter()
        .map(|entry| {
            let token = entry.value();
            let tier_priority = match token.subscription_tier.as_str() {
                "ULTRA" => 3,
                "PRO" => 2,
                _ => 1  // FREE
            };
            let quota_score = token.quota.remaining_fraction;

            // Combined score: tier * 100 + quota percentage
            let score = (tier_priority as f64 * 100.0) + (quota_score * 100.0);

            (token.account_id.clone(), score)
        })
        .collect();

    accounts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    accounts.into_iter().map(|(id, _)| id).collect()
}
```

---

#### FR-5: Quota Cache Management

**Priority**: P1 (High)

**Description**: Efficient in-memory caching of quota information to minimize API calls.

**Acceptance Criteria**:
- [ ] In-memory quota cache (DashMap)
- [ ] Cache key: `account_id:model`
- [ ] TTL: 5 minutes
- [ ] Atomic cache updates
- [ ] Cache invalidation on 429 errors

**Data Structure**:
```rust
pub struct QuotaCache {
    // Key: "account_id:model" -> QuotaInfo
    cache: Arc<DashMap<String, CachedQuotaInfo>>,
}

struct CachedQuotaInfo {
    remaining_fraction: f64,
    reset_time: SystemTime,
    fetched_at: SystemTime,
    ttl: Duration,
}
```

---

### Non-Functional Requirements

#### NFR-1: Performance

**Targets**:
- Quota check latency: <50ms (cached), <200ms (API call)
- Background monitoring overhead: <1% CPU
- Memory footprint: <10MB for quota cache
- No blocking of request flow

#### NFR-2: Reliability

**Targets**:
- API fetch success rate: >99%
- Fallback on quota API failures
- Graceful degradation if quota monitoring unavailable
- Cache persistence across restarts (optional)

#### NFR-3: Observability

**Logging Requirements**:
- Quota fetch operations (INFO level)
- Pre-request quota checks (DEBUG level)
- Quota warnings (WARN level)
- Cache hits/misses (TRACE level)

**Metrics**:
- Quota utilization per account
- 429 error rate before/after
- Account switch frequency
- Quota refresh success rate

---

## User Stories

### Epic: Proactive Quota Monitoring

**Epic ID**: QUOTA-001
**Description**: Implement proactive quota monitoring to minimize 429 errors and improve user experience.

---

#### Story 1: Quota API Integration

**Story ID**: QUOTA-001-01
**Priority**: P0
**Estimate**: 5 points

**As a** system administrator
**I want** the proxy to fetch real-time quota information from Google's API
**So that** we can make informed decisions about account selection

**Acceptance Criteria**:
- [ ] Implement `fetch_available_models()` function
- [ ] Parse `remainingFraction` and `resetTime` per model
- [ ] Handle authentication with Bearer tokens
- [ ] Return structured quota data
- [ ] Unit tests with mock responses

**Technical Reference**: [quota-apis.md#fetch-available-models](../antigravity/api/quota-apis.md#1-fetch-available-models-with-quota-info)

---

#### Story 2: Pre-Request Quota Validation

**Story ID**: QUOTA-001-02
**Priority**: P0
**Estimate**: 3 points

**As a** proxy user
**I want** the system to check quota before making requests
**So that** I don't encounter unnecessary 429 errors

**Acceptance Criteria**:
- [ ] Check quota before each API request
- [ ] Threshold-based decision making (>10% = proceed, <10% = switch)
- [ ] Log quota warnings
- [ ] Automatic account switching when quota low
- [ ] Integration tests

**Technical Reference**: [quota-apis.md#monitoring-strategies](../antigravity/api/quota-apis.md#monitoring-strategies)

---

#### Story 3: Background Quota Monitoring

**Story ID**: QUOTA-001-03
**Priority**: P1
**Estimate**: 5 points

**As a** system
**I want** to periodically refresh quota information in the background
**So that** quota data remains up-to-date without blocking requests

**Acceptance Criteria**:
- [ ] Background task runs every 5 minutes
- [ ] Refreshes quotas for all accounts
- [ ] Non-blocking execution
- [ ] Configurable refresh interval
- [ ] Error handling and logging

---

#### Story 4: Subscription Tier Detection

**Story ID**: QUOTA-001-04
**Priority**: P1
**Estimate**: 3 points

**As a** system
**I want** to detect and prioritize accounts by subscription tier
**So that** higher-tier accounts (with better quotas) are used first

**Acceptance Criteria**:
- [ ] Implement `load_code_assist()` API call
- [ ] Parse subscription tier (FREE/PRO/ULTRA)
- [ ] Store tier per account
- [ ] Prioritize accounts: ULTRA > PRO > FREE
- [ ] Update account selection algorithm

**Technical Reference**: [quota-apis.md#subscription-tiers](../antigravity/api/quota-apis.md#subscription-tiers)

---

#### Story 5: Quota Cache Implementation

**Story ID**: QUOTA-001-05
**Priority**: P1
**Estimate**: 3 points

**As a** system
**I want** to cache quota information efficiently
**So that** we minimize redundant API calls

**Acceptance Criteria**:
- [ ] In-memory quota cache with DashMap
- [ ] 5-minute TTL per cache entry
- [ ] Cache key: `account_id:model`
- [ ] Atomic cache updates
- [ ] Cache invalidation on 429 errors

---

#### Story 6: Quota Health UI Indicators

**Story ID**: QUOTA-001-06
**Priority**: P2
**Estimate**: 3 points

**As a** user
**I want** to see quota health status in the UI
**So that** I'm aware of quota consumption and upcoming resets

**Acceptance Criteria**:
- [ ] Display `remainingFraction` percentage per account
- [ ] Color-coded health indicators (green/yellow/red)
- [ ] Show `resetTime` in local timezone
- [ ] Real-time updates from background monitoring

---

## Technical Architecture

### Component Diagram

```
┌─────────────────────────────────────────────────┐
│         Antigravity Manager Proxy               │
│                                                 │
│  ┌─────────────────────────────────────────┐   │
│  │       Request Handler                   │   │
│  │  ┌──────────────────────────────────┐   │   │
│  │  │  1. Pre-Request Quota Check      │   │   │
│  │  │     - Check cache                │   │   │
│  │  │     - Validate remainingFraction │   │   │
│  │  │     - Account selection          │   │   │
│  │  └──────────────────────────────────┘   │   │
│  └─────────────────────────────────────────┘   │
│                    ↓                            │
│  ┌─────────────────────────────────────────┐   │
│  │       Quota Manager                     │   │
│  │  ┌──────────────────────────────────┐   │   │
│  │  │  Quota Cache (DashMap)           │   │   │
│  │  │  Key: "account:model"            │   │   │
│  │  │  Value: QuotaInfo + TTL          │   │   │
│  │  └──────────────────────────────────┘   │   │
│  │  ┌──────────────────────────────────┐   │   │
│  │  │  Background Monitor Task         │   │   │
│  │  │  - Tokio interval (5 min)        │   │   │
│  │  │  - Fetch quotas for all accounts │   │   │
│  │  │  - Update cache atomically       │   │   │
│  │  └──────────────────────────────────┘   │   │
│  └─────────────────────────────────────────┘   │
│                    ↓                            │
│  ┌─────────────────────────────────────────┐   │
│  │    Google Antigravity v1internal API    │   │
│  │  - fetchAvailableModels                 │   │
│  │  - loadCodeAssist                       │   │
│  └─────────────────────────────────────────┘   │
└─────────────────────────────────────────────────┘
```

### Data Flow

**1. Request Flow with Quota Check**:
```
Incoming API Request
  ↓
Pre-Request Quota Check:
  ├─→ Check Cache (account:model)
  ├─→ If cached and fresh:
  │     ├─→ remainingFraction > 0.10: Proceed
  │     ├─→ remainingFraction 0.05-0.10: Log warning, proceed
  │     └─→ remainingFraction < 0.05: Switch account
  └─→ If cache miss or stale:
        └─→ Fetch from API
              └─→ Update cache
                    └─→ Retry quota check
```

**2. Background Monitoring Flow**:
```
Tokio Interval (5 min):
  ↓
For each account:
  ├─→ Call fetchAvailableModels(access_token, project_id)
  ├─→ Parse response: {models: {quotaInfo}}
  ├─→ For each model:
  │     └─→ Update cache[account:model] = QuotaInfo
  └─→ Log refresh status
```

---

## Implementation Plan

### Phase 1: Core Quota APIs (Week 1)
- Story QUOTA-001-01: Quota API integration
- Story QUOTA-001-05: Quota cache implementation

### Phase 2: Pre-Request Validation (Week 2)
- Story QUOTA-001-02: Pre-request quota validation
- Story QUOTA-001-04: Subscription tier detection

### Phase 3: Background Monitoring (Week 3)
- Story QUOTA-001-03: Background quota monitoring
- Integration testing and bug fixes

### Phase 4: UI & Observability (Week 4)
- Story QUOTA-001-06: Quota health UI indicators
- Metrics and logging improvements
- Documentation updates

---

## Dependencies

### External Dependencies
- Google Antigravity v1internal API availability
- OAuth authentication working correctly
- Network connectivity to Google APIs

### Internal Dependencies
- TokenManager module
- RateLimitTracker module
- Tauri event system (for UI updates)

---

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| API rate limiting on quota checks | Medium | Medium | Cache quota data (5 min TTL), batch requests |
| Quota API unavailable | Low | High | Fallback to reactive 429 handling |
| Cache synchronization issues | Medium | Low | Use atomic DashMap operations |
| Subscription tier detection failures | Low | Medium | Default to FREE tier assumptions |
| Background task CPU overhead | Low | Low | Configurable interval, async execution |

---

## Success Criteria

**Launch Criteria**:
- [ ] 429 error rate reduced to <3%
- [ ] Pre-request quota validation working for all models
- [ ] Background monitoring running stably for 48 hours
- [ ] Subscription tier detection accuracy >95%
- [ ] No performance regression (latency increase <5%)

**Post-Launch Metrics** (30 days):
- 429 error rate: <3% (vs. current 15-20%)
- Account switch latency: <500ms (vs. current 2-5s)
- API call success rate: >95% (vs. current 70-80%)
- User-reported quota issues: <5 per week

---

## Related Documentation

**Reverse Engineering References**:
- [Quota Management APIs](../antigravity/api/quota-apis.md) - Complete API documentation
- [Error Pattern Catalog](../antigravity/reference/error-pattern-catalog.md) - 429 error handling
- [Complete Examples](../antigravity/examples/complete-examples.md) - Request/response examples

**Architecture Documentation**:
- [v1internal API Specification](../antigravity/architecture/v1internal-api.md)
- [OAuth Flow](../antigravity/authentication/oauth-flow.md)

**Epic & Stories**:
- [Epic: Proactive Quota Monitoring](../epics/proactive-quota-monitoring-epic.md)
- [User Stories](../stories/)

---

**Document History**:
- 2026-01-10: Initial draft (v1.0.0)
