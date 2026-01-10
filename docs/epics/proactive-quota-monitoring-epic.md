# Epic: Proactive Quota Monitoring System

**Epic ID**: QUOTA-001
**Status**: Planning
**Priority**: P0 (Critical)
**Target Release**: v3.4.0
**Created**: 2026-01-10
**Owner**: Engineering Team

---

## Epic Overview

### Problem Statement

Current reactive quota management causes **15-20% 429 error rate**, resulting in:

- ❌ **Poor UX**: Interrupted workflows from unexpected quota exhaustion
- ❌ **Resource Waste**: API calls to accounts with exhausted quotas
- ❌ **Slow Recovery**: 2-5 second delays when switching accounts
- ❌ **Limited Visibility**: Users unaware of approaching quota limits

### Solution Vision

Implement **proactive quota monitoring** to:

- ✅ **Prevent 429 errors** before they occur (<3% target error rate)
- ✅ **Real-time quota tracking** via Google's `fetchAvailableModels` API
- ✅ **Intelligent account selection** based on quota availability and subscription tiers
- ✅ **Predictive warnings** when quota approaches limits
- ✅ **Background monitoring** with 5-minute refresh intervals

### Success Metrics

| Metric | Baseline | Target | Method |
|--------|----------|--------|--------|
| **429 Error Rate** | 15-20% | <3% | Error logs analysis |
| **Account Switch Latency** | 2-5s | <500ms | Performance metrics |
| **API Success Rate** | 70-80% | >95% | Request analytics |
| **User-Reported Issues** | ~10/week | <5/week | Support tickets |

---

## Business Value

### User Impact

**Primary Benefits**:
- Uninterrupted AI coding workflows
- Faster response times (no retry delays)
- Transparent quota visibility

**Secondary Benefits**:
- Reduced frustration from failed requests
- Better understanding of account quota consumption
- Predictable service availability

### Technical Impact

**System Improvements**:
- 80% reduction in wasted API calls
- Faster account rotation (<500ms vs 2-5s)
- Better resource utilization across accounts

**Code Quality**:
- Cleaner error handling (proactive vs reactive)
- Improved observability (quota metrics)
- More resilient architecture

---

## Scope

### In Scope

✅ **Core Quota Monitoring**:
- Integration with `fetchAvailableModels` API
- Pre-request quota validation
- Background quota refresh task
- Quota cache management (DashMap)

✅ **Intelligent Account Selection**:
- Subscription tier detection (FREE/PRO/ULTRA)
- Tier-based account prioritization
- Quota-aware account rotation

✅ **Observability**:
- Quota health logging
- Metrics for quota consumption
- UI quota indicators (basic)

### Out of Scope

❌ **Advanced Features** (Future releases):
- Quota forecasting/prediction
- Per-user quota limits
- Custom quota allocation rules
- Historical quota analytics dashboard

❌ **Third-Party Integrations**:
- Non-Google AI providers
- Custom quota APIs

---

## Technical Architecture

### High-Level Design

```
┌───────────────────────────────────────────────────┐
│         Antigravity Manager Proxy                 │
│                                                   │
│  ┌─────────────────────────────────────────┐     │
│  │  Request Flow                           │     │
│  │  ┌───────────────────────────────────┐  │     │
│  │  │ 1. Pre-Request Quota Check        │  │     │
│  │  │    - Query QuotaCache             │  │     │
│  │  │    - Validate remainingFraction   │  │     │
│  │  │    - Select best account          │  │     │
│  │  └───────────────────────────────────┘  │     │
│  └─────────────────────────────────────────┘     │
│                     ↓                             │
│  ┌─────────────────────────────────────────┐     │
│  │  Quota Manager (New Module)             │     │
│  │                                         │     │
│  │  Components:                            │     │
│  │  - QuotaCache (DashMap)                 │     │
│  │  - QuotaFetcher (API client)            │     │
│  │  - BackgroundMonitor (Tokio task)       │     │
│  │  - TierDetector (Subscription logic)    │     │
│  └─────────────────────────────────────────┘     │
│                     ↓                             │
│  ┌─────────────────────────────────────────┐     │
│  │  Google Antigravity v1internal API      │     │
│  │  - fetchAvailableModels                 │     │
│  │  - loadCodeAssist                       │     │
│  └─────────────────────────────────────────┘     │
└───────────────────────────────────────────────────┘
```

### New Components

**1. QuotaManager Module** (`src-tauri/src/proxy/quota_manager.rs`):
```rust
pub struct QuotaManager {
    /// Quota cache: "account:model" -> QuotaInfo
    cache: Arc<DashMap<String, CachedQuotaInfo>>,

    /// API client for fetching quotas
    fetcher: QuotaFetcher,

    /// Background monitoring task handle
    monitor_task: Option<tokio::task::JoinHandle<()>>,

    /// Subscription tier cache
    tier_cache: Arc<DashMap<String, SubscriptionTier>>,
}
```

**2. QuotaFetcher** (API Client):
```rust
impl QuotaFetcher {
    pub async fn fetch_available_models(
        &self,
        access_token: &str,
        project_id: &str
    ) -> Result<HashMap<String, QuotaInfo>, String>;

    pub async fn load_code_assist(
        &self,
        access_token: &str
    ) -> Result<SubscriptionInfo, String>;
}
```

**3. BackgroundMonitor** (Tokio Task):
```rust
async fn background_quota_monitor(
    tokens: Arc<DashMap<String, ProxyToken>>,
    cache: Arc<DashMap<String, CachedQuotaInfo>>,
    fetcher: QuotaFetcher
) {
    let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 min

    loop {
        interval.tick().await;
        refresh_all_quotas(&tokens, &cache, &fetcher).await;
    }
}
```

---

## Dependencies

### External Dependencies

| Dependency | Type | Risk Level | Mitigation |
|------------|------|------------|------------|
| Google Antigravity `fetchAvailableModels` API | Critical | Medium | Cache quotas, fallback to reactive mode |
| Google Antigravity `loadCodeAssist` API | Important | Low | Default to FREE tier if unavailable |
| OAuth token validity | Critical | Medium | Token refresh on 401 errors |

### Internal Dependencies

| Module | Dependency Type | Impact |
|--------|----------------|--------|
| `TokenManager` | Integration | Must pass account info to QuotaManager |
| `RateLimitTracker` | Complementary | Works alongside (not replaced) |
| Tauri event system | Optional | For UI quota updates |

---

## User Stories

### Story Breakdown

**Total Stories**: 6
**Total Estimate**: 22 story points

#### P0 Stories (Must Have)

**[QUOTA-001-01] Quota API Integration** (5 points)
- Implement `fetch_available_models()` API client
- Parse `remainingFraction` and `resetTime`
- Handle authentication and errors
- **Reference**: [quota-apis.md](../antigravity/api/quota-apis.md#1-fetch-available-models-with-quota-info)

**[QUOTA-001-02] Pre-Request Quota Validation** (3 points)
- Check quota before API requests
- Threshold-based decision making (>10%, <10%, 0%)
- Automatic account switching
- **Reference**: [quota-apis.md](../antigravity/api/quota-apis.md#monitoring-strategies)

**[QUOTA-001-05] Quota Cache Implementation** (3 points)
- In-memory cache with DashMap
- 5-minute TTL per entry
- Atomic cache updates

#### P1 Stories (Should Have)

**[QUOTA-001-03] Background Quota Monitoring** (5 points)
- Background task with 5-minute interval
- Refresh quotas for all accounts
- Non-blocking execution

**[QUOTA-001-04] Subscription Tier Detection** (3 points)
- Implement `load_code_assist()` API call
- Detect FREE/PRO/ULTRA tiers
- Prioritize higher-tier accounts
- **Reference**: [quota-apis.md](../antigravity/api/quota-apis.md#subscription-tiers)

#### P2 Stories (Nice to Have)

**[QUOTA-001-06] Quota Health UI Indicators** (3 points)
- Display `remainingFraction` percentage
- Color-coded health indicators
- Show `resetTime` in local timezone

---

## Implementation Plan

### Phase 1: Foundation (Week 1)

**Stories**: QUOTA-001-01, QUOTA-001-05
**Goal**: Core API integration and caching

**Deliverables**:
- [ ] `QuotaFetcher` API client
- [ ] `QuotaCache` implementation
- [ ] Unit tests with mock API responses
- [ ] Integration tests

**Technical Tasks**:
1. Create `src-tauri/src/proxy/quota_manager.rs`
2. Implement `fetch_available_models()` API call
3. Implement `QuotaCache` with DashMap
4. Add data structures (`QuotaInfo`, `CachedQuotaInfo`)
5. Write unit tests

---

### Phase 2: Request Integration (Week 2)

**Stories**: QUOTA-001-02, QUOTA-001-04
**Goal**: Pre-request validation and tier detection

**Deliverables**:
- [ ] Pre-request quota check in `TokenManager::get_token()`
- [ ] Subscription tier detection via `loadCodeAssist`
- [ ] Tier-based account prioritization
- [ ] Integration tests

**Technical Tasks**:
1. Add pre-request quota check to `get_token()`
2. Implement threshold logic (>10%, <10%, 0%)
3. Implement `load_code_assist()` API call
4. Update account selection algorithm
5. Add logging and metrics

---

### Phase 3: Background Monitoring (Week 3)

**Stories**: QUOTA-001-03
**Goal**: Automated quota refresh

**Deliverables**:
- [ ] Background Tokio task
- [ ] Configurable refresh interval
- [ ] Error handling and recovery
- [ ] Stability testing (48-hour run)

**Technical Tasks**:
1. Implement background monitoring task
2. Add configuration for refresh interval
3. Handle API failures gracefully
4. Add monitoring metrics
5. Performance testing

---

### Phase 4: UI & Polish (Week 4)

**Stories**: QUOTA-001-06
**Goal**: User-visible improvements

**Deliverables**:
- [ ] Quota health UI indicators
- [ ] Documentation updates
- [ ] Performance optimization
- [ ] Bug fixes

**Technical Tasks**:
1. Add quota display to account UI
2. Implement color-coded health indicators
3. Update user documentation
4. Performance profiling
5. Bug fixes and polish

---

## Testing Strategy

### Unit Tests

**Coverage Target**: >80%

**Test Cases**:
- QuotaFetcher API parsing
- QuotaCache operations (get, set, invalidate)
- Tier detection logic
- Threshold-based decision making

### Integration Tests

**Scenarios**:
- End-to-end quota fetch and cache update
- Pre-request quota check with account switching
- Background monitoring task execution
- API error handling (401, 403, 429, 500)

### Performance Tests

**Benchmarks**:
- Quota check latency: <50ms (cached), <200ms (API)
- Background task CPU usage: <1%
- Memory footprint: <10MB
- Concurrent request handling: 100 req/s

### Manual Testing

**Test Plan**:
- [ ] Verify quota fetch from Google API
- [ ] Test account switching on low quota
- [ ] Verify background monitoring stability (48h)
- [ ] Test UI quota indicators
- [ ] Stress test with multiple accounts

---

## Rollout Plan

### Phase 1: Internal Testing (Week 1-2)

**Participants**: Engineering team
**Deployment**: Local development environment
**Goal**: Validate core functionality

**Success Criteria**:
- [ ] All unit tests passing
- [ ] Integration tests passing
- [ ] No performance regressions

### Phase 2: Beta Testing (Week 3)

**Participants**: Select users (5-10)
**Deployment**: Beta channel
**Goal**: Real-world validation

**Success Criteria**:
- [ ] 429 error rate <5% (improvement from 15-20%)
- [ ] No crashes or critical bugs
- [ ] Positive user feedback

### Phase 3: General Availability (Week 4)

**Participants**: All users
**Deployment**: Stable channel (v3.4.0)
**Goal**: Full rollout

**Success Criteria**:
- [ ] 429 error rate <3%
- [ ] Background monitoring running stably
- [ ] <5 user-reported issues per week

---

## Risk Assessment

### High-Risk Items

**Risk 1: API Rate Limiting on Quota Checks**
- **Probability**: Medium
- **Impact**: Medium
- **Mitigation**: Cache quota data (5-min TTL), batch API requests
- **Contingency**: Increase cache TTL, reduce refresh frequency

**Risk 2: Quota API Unavailable**
- **Probability**: Low
- **Impact**: High
- **Mitigation**: Fallback to reactive 429 handling
- **Contingency**: Continue using existing rate limit tracking

### Medium-Risk Items

**Risk 3: Cache Synchronization Issues**
- **Probability**: Medium
- **Impact**: Low
- **Mitigation**: Use atomic DashMap operations
- **Contingency**: Add cache locking mechanisms

**Risk 4: Background Task CPU Overhead**
- **Probability**: Low
- **Impact**: Low
- **Mitigation**: Configurable refresh interval, async execution
- **Contingency**: Reduce refresh frequency, disable if needed

---

## Metrics & Monitoring

### Key Performance Indicators (KPIs)

**Primary KPIs**:
- **429 Error Rate**: Target <3% (current: 15-20%)
- **Account Switch Latency**: Target <500ms (current: 2-5s)
- **API Success Rate**: Target >95% (current: 70-80%)

**Secondary KPIs**:
- Quota fetch success rate: >99%
- Cache hit rate: >90%
- Background task uptime: >99.5%

### Logging

**Log Levels**:
- **INFO**: Quota fetch operations, account switches
- **WARN**: Low quota warnings, API failures
- **DEBUG**: Pre-request quota checks, cache operations
- **ERROR**: API errors, cache failures

**Log Examples**:
```
[INFO] [Quota-Fetch] Fetched quotas for account@gmail.com: 11 models
[WARN] [Quota-Warning] Account account@gmail.com model claude-opus: 5% remaining
[INFO] [Account-Switch] Switched from account1 to account2 due to low quota
```

### Metrics

**Prometheus Metrics** (future):
- `quota_remaining_fraction{account, model}`
- `quota_fetch_duration_seconds`
- `quota_cache_hit_ratio`
- `account_switch_count`

---

## Documentation

### User Documentation

**Required Updates**:
- [ ] Update README with quota monitoring feature
- [ ] Add quota management guide
- [ ] Document quota health indicators in UI
- [ ] Update troubleshooting section

### Technical Documentation

**New Documentation**:
- [ ] Quota Manager API reference
- [ ] Architecture decision record (ADR)
- [ ] Deployment guide for quota monitoring
- [ ] Performance tuning guide

### Reverse Engineering References

**Key Documents**:
- [Quota Management APIs](../antigravity/api/quota-apis.md)
- [Error Pattern Catalog](../antigravity/reference/error-pattern-catalog.md)
- [Complete Examples](../antigravity/examples/complete-examples.md)
- [Subscription Tiers](../antigravity/api/quota-apis.md#subscription-tiers)

---

## Success Criteria

### Launch Criteria

**Must Have (P0)**:
- [ ] 429 error rate reduced to <5% (beta target)
- [ ] Pre-request quota validation functional
- [ ] Background monitoring running for 48+ hours without issues
- [ ] No performance regression (latency <5% increase)
- [ ] All P0 stories completed

**Should Have (P1)**:
- [ ] Subscription tier detection working
- [ ] Quota cache hit rate >80%
- [ ] All integration tests passing

### Post-Launch Success (30 days)

**Quantitative Metrics**:
- [ ] 429 error rate: <3%
- [ ] Account switch latency: <500ms
- [ ] API success rate: >95%
- [ ] User-reported issues: <5/week

**Qualitative Metrics**:
- [ ] Positive user feedback on improved reliability
- [ ] No critical bugs reported
- [ ] Engineering team satisfied with code quality

---

## Related Documents

**Product Requirements**:
- [Proactive Quota Monitoring PRD](../product-requirements/proactive-quota-monitoring-prd.md)

**User Stories**:
- [Story QUOTA-001-01](../stories/QUOTA-001-01-quota-api-integration.md)
- [Story QUOTA-001-02](../stories/QUOTA-001-02-pre-request-validation.md)
- [Story QUOTA-001-03](../stories/QUOTA-001-03-background-monitoring.md)
- [Story QUOTA-001-04](../stories/QUOTA-001-04-subscription-tier.md)
- [Story QUOTA-001-05](../stories/QUOTA-001-05-quota-cache.md)
- [Story QUOTA-001-06](../stories/QUOTA-001-06-ui-indicators.md)

**Technical References**:
- [Google Antigravity Reverse Engineering](../antigravity/README.md)
- [Architecture Overview](../architecture/system-overview.md)

---

**Epic History**:
- 2026-01-10: Epic created (Planning phase)
