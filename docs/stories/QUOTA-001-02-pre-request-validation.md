# Story QUOTA-001-02: Pre-Request Quota Validation

**Story ID**: QUOTA-001-02
**Epic**: [QUOTA-001 - Proactive Quota Monitoring](../epics/proactive-quota-monitoring-epic.md)
**Priority**: P0 (Critical)
**Estimate**: 3 story points
**Status**: To Do
**Assignee**: TBD
**Dependencies**: QUOTA-001-01 (Quota API Integration)

---

## User Story

**As a** proxy user
**I want** the system to check quota availability before making API requests
**So that** I don't encounter unnecessary 429 errors and experience smooth workflows

---

## Context

After implementing quota fetching (QUOTA-001-01), we need to **use** this quota data to make intelligent decisions. Instead of blindly sending requests and handling 429 errors reactively, we should:

1. Check `remainingFraction` **before** the request
2. Switch accounts if quota is low (<10%)
3. Only proceed if quota is healthy (>10%)

**Impact**: This should reduce 429 errors from **15-20%** to **<3%**.

---

## Technical Details

### Implementation Location

**File**: `src-tauri/src/proxy/token_manager.rs`
**Function**: `TokenManager::get_token()`

**Reference**: [quota-apis.md#monitoring-strategies](../antigravity/api/quota-apis.md#monitoring-strategies)

### Quota Check Logic

**Thresholds** ([quota-apis.md#best-practices](../antigravity/api/quota-apis.md#best-practices)):
```yaml
quota_health_levels:
  healthy: "> 0.20 (20%)"      # Proceed normally
  warning: "0.10-0.20 (10-20%)" # Log warning, proceed
  critical: "0.05-0.10 (5-10%)" # Consider switching
  exhausted: "< 0.05 (5%)"      # Switch account immediately
```

**Decision Flow**:
```rust
pub async fn get_token(
    &self,
    quota_group: &str,
    force_rotate: bool,
    session_id: Option<&str>,
    model: Option<&str>,
) -> Result<(String, String, String), String> {
    // ... existing sticky session logic ...

    for candidate in weighted_candidates {
        // 🆕 PRE-REQUEST QUOTA CHECK
        if let Some(m) = model {
            let quota = self.quota_manager.get_quota(&candidate.account_id, m).await;

            match quota {
                Some(qi) if qi.remaining_fraction < 0.05 => {
                    tracing::warn!(
                        "⚠️ [Quota-Skip] Account {} model {} quota critical: {:.1}%, skipping",
                        candidate.account_id,
                        m,
                        qi.remaining_fraction * 100.0
                    );
                    continue;  // Skip this account
                }
                Some(qi) if qi.remaining_fraction < 0.10 => {
                    tracing::warn!(
                        "⚠️ [Quota-Warning] Account {} model {} quota low: {:.1}%",
                        candidate.account_id,
                        m,
                        qi.remaining_fraction * 100.0
                    );
                    // Proceed but warn
                }
                Some(qi) => {
                    tracing::debug!(
                        "✅ [Quota-OK] Account {} model {} quota healthy: {:.1}%",
                        candidate.account_id,
                        m,
                        qi.remaining_fraction * 100.0
                    );
                }
                None => {
                    // No quota info - proceed with caution
                    tracing::debug!("[Quota-Unknown] No quota data for {}", candidate.account_id);
                }
            }
        }

        // 🆕 EXISTING RATE LIMIT CHECK (keep this!)
        let is_limited = if let Some(m) = model {
            self.rate_limit_tracker.is_rate_limited_for_model(&candidate.account_id, m)
        } else {
            self.is_rate_limited(&candidate.account_id)
        };

        if is_limited {
            continue;
        }

        // Account passed all checks - use it
        return Ok((
            candidate.access_token.clone(),
            candidate.project_id.clone(),
            candidate.account_id.clone()
        ));
    }

    Err("All accounts unavailable (rate limited or quota exhausted)".to_string())
}
```

---

## Acceptance Criteria

### AC-1: Pre-Request Quota Check

**Given** a request for model "claude-opus-4-5-thinking"
**When** `get_token()` is called
**Then** the system should:
- [ ] Query quota cache for each candidate account
- [ ] Skip accounts with `remainingFraction < 0.05`
- [ ] Log warnings for accounts with `remainingFraction < 0.10`
- [ ] Select first account with healthy quota
- [ ] Fall back to next account if primary has low quota

---

### AC-2: Threshold-Based Decision Making

**Test Cases**:

**Test 1: Healthy Quota**
```rust
#[tokio::test]
async fn test_healthy_quota_proceeds() {
    let manager = setup_manager_with_quota(0.85);  // 85% remaining

    let result = manager.get_token("agent", false, None, Some("claude-opus-4-5-thinking")).await;

    assert!(result.is_ok());
    // Should use account with 85% quota
}
```

**Test 2: Low Quota**
```rust
#[tokio::test]
async fn test_low_quota_switches_account() {
    let manager = setup_manager_with_quotas(vec![
        ("acc1", 0.03),  // 3% - too low
        ("acc2", 0.75)   // 75% - healthy
    ]);

    let result = manager.get_token("agent", false, None, Some("claude-opus-4-5-thinking")).await;

    assert!(result.is_ok());
    let (_, _, account_id) = result.unwrap();
    assert_eq!(account_id, "acc2");  // Should skip acc1, use acc2
}
```

**Test 3: All Accounts Exhausted**
```rust
#[tokio::test]
async fn test_all_quotas_exhausted() {
    let manager = setup_manager_with_quotas(vec![
        ("acc1", 0.01),  // 1%
        ("acc2", 0.02)   // 2%
    ]);

    let result = manager.get_token("agent", false, None, Some("claude-opus-4-5-thinking")).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("quota exhausted"));
}
```

---

### AC-3: Logging and Observability

**Given** quota checks during request processing
**When** various quota levels are encountered
**Then** the system should:
- [ ] Log INFO when account selected with healthy quota
- [ ] Log WARN when quota <10% but proceeding
- [ ] Log WARN when skipping account due to low quota
- [ ] Include quota percentage in logs

**Log Examples**:
```
[INFO] ✅ [Quota-OK] Account acc@gmail.com model claude-opus quota healthy: 85.0%
[WARN] ⚠️ [Quota-Warning] Account acc@gmail.com model claude-opus quota low: 7.5%
[WARN] ⚠️ [Quota-Skip] Account acc@gmail.com model claude-opus quota critical: 2.1%, skipping
```

---

### AC-4: Integration with Existing Rate Limit System

**Given** both quota check and rate limit check
**When** selecting an account
**Then** the system should:
- [ ] Check quota first (proactive)
- [ ] Check rate limits second (existing logic)
- [ ] Skip account if either check fails
- [ ] Both systems work independently

**Validation**:
- Quota check does NOT replace rate limit tracking
- Both checks must pass for account to be selected
- Rate limit tracking continues to handle 429 errors

---

## Implementation Tasks

### Task 1: Add QuotaManager to TokenManager

**File**: `src-tauri/src/proxy/token_manager.rs`

```rust
pub struct TokenManager {
    // ... existing fields ...

    // 🆕 Quota manager
    quota_manager: Arc<QuotaManager>,
}

impl TokenManager {
    pub fn new(/* ... */, quota_manager: Arc<QuotaManager>) -> Self {
        Self {
            // ... existing fields ...
            quota_manager,
        }
    }
}
```

**Estimated Effort**: 30 minutes

---

### Task 2: Implement Pre-Request Quota Check

**Add to** `get_token()` function before rate limit check:

```rust
// 🆕 Quota Check (before rate limit check)
if let Some(m) = model {
    if let Some(quota_info) = self.quota_manager.get_quota(&candidate.account_id, m).await {
        if quota_info.remaining_fraction < 0.05 {
            tracing::warn!(
                "⚠️ [Quota-Skip] Account {} model {} quota critical: {:.1}%, skipping",
                candidate.account_id,
                m,
                quota_info.remaining_fraction * 100.0
            );
            continue;
        } else if quota_info.remaining_fraction < 0.10 {
            tracing::warn!(
                "⚠️ [Quota-Warning] Account {} model {} quota low: {:.1}%",
                candidate.account_id,
                m,
                quota_info.remaining_fraction * 100.0
            );
        }
    }
}

// Existing rate limit check (keep this!)
let is_limited = if let Some(m) = model {
    self.rate_limit_tracker.is_rate_limited_for_model(&candidate.account_id, m)
} else {
    self.is_rate_limited(&candidate.account_id)
};

if is_limited {
    continue;
}
```

**Estimated Effort**: 1 hour

---

### Task 3: Unit Tests

**Test Cases**:
```rust
#[tokio::test]
async fn test_skip_low_quota_account();

#[tokio::test]
async fn test_warn_on_moderate_quota();

#[tokio::test]
async fn test_quota_and_rate_limit_both_checked();

#[tokio::test]
async fn test_no_quota_data_proceeds_anyway();
```

**Estimated Effort**: 2 hours

---

## Verification

### Manual Testing Checklist

- [ ] Start proxy with multiple accounts
- [ ] Monitor logs during API requests
- [ ] Verify quota checks appear in logs
- [ ] Confirm account switching on low quota
- [ ] Validate 429 error rate decrease

### Performance Testing

- [ ] Measure latency impact of quota checks
- [ ] Verify cache hit rate >80%
- [ ] Ensure no blocking of request flow

---

## Rollback Plan

**If issues occur**:
1. Feature flag to disable quota checks
2. Revert to reactive 429 handling
3. Keep quota cache for observability only

**Rollback Trigger**:
- Performance regression >10%
- Increased error rates
- Critical bugs affecting availability

---

## Related Documents

**Epic**: [QUOTA-001 - Proactive Quota Monitoring](../epics/proactive-quota-monitoring-epic.md)

**Reverse Engineering References**:
- [Quota Monitoring Strategies](../antigravity/api/quota-apis.md#monitoring-strategies)
- [Best Practices](../antigravity/api/quota-apis.md#best-practices)

**Related Stories**:
- [QUOTA-001-01: Quota API Integration](QUOTA-001-01-quota-api-integration.md) (prerequisite)
- [QUOTA-001-03: Background Monitoring](QUOTA-001-03-background-monitoring.md) (uses same cache)

---

**Story History**:
- 2026-01-10: Story created
