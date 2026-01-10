# Story QUOTA-001-05: Quota Cache Implementation

**Story ID**: QUOTA-001-05
**Epic**: [QUOTA-001](../epics/proactive-quota-monitoring-epic.md)
**Priority**: P1
**Estimate**: 3 points
**Status**: To Do

---

## User Story

**As a** system
**I want** efficient in-memory caching of quota information
**So that** we minimize redundant API calls to Google

---

## Acceptance Criteria

### AC-1: Cache Implementation

- [ ] DashMap for thread-safe cache
- [ ] Key format: `"account_id:model"`
- [ ] TTL: 5 minutes per entry
- [ ] Atomic updates

**Data Structure**:
```rust
pub struct QuotaCache {
    cache: Arc<DashMap<String, CachedQuotaInfo>>,
}

#[derive(Debug, Clone)]
pub struct CachedQuotaInfo {
    pub remaining_fraction: f64,
    pub reset_time: SystemTime,
    pub fetched_at: SystemTime,
    pub ttl: Duration,
}

impl QuotaCache {
    pub fn get(&self, account_id: &str, model: &str) -> Option<CachedQuotaInfo>;
    pub fn set(&self, account_id: &str, model: &str, info: QuotaInfo);
    pub fn is_fresh(&self, info: &CachedQuotaInfo) -> bool;
    pub fn invalidate(&self, account_id: &str, model: &str);
}
```

---

### AC-2: Cache Operations

- [ ] `get()`: Return quota if fresh, None if stale/missing
- [ ] `set()`: Store quota with timestamp
- [ ] `is_fresh()`: Check if fetched_at + TTL > now
- [ ] `invalidate()`: Remove on 429 errors

---

### AC-3: Performance

- [ ] Cache lookups: <1ms
- [ ] No memory leaks (stable memory usage)
- [ ] Cache hit rate: >80% (after warmup)

---

## Implementation

**File**: `src-tauri/src/proxy/quota_cache.rs`

**Estimated Effort**: 4 hours

---

## Testing

**Unit Tests**:
- [ ] Cache get/set operations
- [ ] TTL expiration logic
- [ ] Thread-safety (concurrent access)

---

## Related Documents

- [Epic QUOTA-001](../epics/proactive-quota-monitoring-epic.md)
- [QUOTA-001-01: API Integration](QUOTA-001-01-quota-api-integration.md)

---

**Story History**:
- 2026-01-10: Story created
