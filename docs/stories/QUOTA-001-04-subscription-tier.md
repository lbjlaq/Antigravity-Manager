# Story QUOTA-001-04: Subscription Tier Detection and Prioritization

**Story ID**: QUOTA-001-04
**Epic**: [QUOTA-001](../epics/proactive-quota-monitoring-epic.md)
**Priority**: P1 (High)
**Estimate**: 3 points
**Status**: To Do

---

## User Story

**As a** system
**I want** to detect and prioritize accounts by subscription tier (FREE/PRO/ULTRA)
**So that** higher-tier accounts with better quotas are used first

---

## Context

**Reverse Engineering Discovery** ([quota-apis.md#subscription-tiers](../antigravity/api/quota-apis.md#subscription-tiers)):

```yaml
subscription_tiers:
  ULTRA:
    quota_limits: "Highest (best for heavy usage)"
    rate_limiting: "Most relaxed"
  PRO:
    quota_limits: "Higher than FREE"
    rate_limiting: "Relaxed"
  FREE:
    quota_limits: "Limited"
    rate_limiting: "Aggressive (more 429 errors)"
```

**Impact**: Prioritizing ULTRA/PRO accounts reduces 429 errors.

---

## Technical Implementation

### API: loadCodeAssist

**Reference**: [quota-apis.md#load-code-assist](../antigravity/api/quota-apis.md#2-load-code-assist-project--subscription-discovery)

**Endpoint**:
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
  "currentTier": {"id": "FREE"},
  "paidTier": {"id": "PRO"}
}
```

**Priority Logic**: Use `paidTier.id` if available, fallback to `currentTier.id`

---

## Acceptance Criteria

### AC-1: Tier Detection

- [ ] Call `loadCodeAssist` during account initialization
- [ ] Parse `paidTier.id` (preferred) or `currentTier.id`
- [ ] Store tier in `ProxyToken` struct
- [ ] Handle API errors (default to FREE)

### AC-2: Account Prioritization

- [ ] Sort accounts by tier: ULTRA > PRO > FREE
- [ ] Within same tier, sort by quota remaining
- [ ] Update `get_token()` selection algorithm

**Algorithm**:
```rust
fn calculate_account_score(account: &ProxyToken, quota: Option<f64>) -> f64 {
    let tier_score = match account.subscription_tier.as_str() {
        "ULTRA" => 300.0,
        "PRO" => 200.0,
        _ => 100.0  // FREE
    };

    let quota_score = quota.unwrap_or(0.5) * 100.0;

    tier_score + quota_score
    // Example: ULTRA with 80% quota = 300 + 80 = 380
    //          PRO with 90% quota = 200 + 90 = 290
    //          FREE with 100% quota = 100 + 100 = 200
}
```

### AC-3: Testing

- [ ] Unit tests for tier detection
- [ ] Unit tests for prioritization algorithm
- [ ] Integration test with mixed tier accounts

---

## Implementation Tasks

1. Add `subscription_tier` field to `ProxyToken` struct
2. Implement `load_code_assist()` API call
3. Update account initialization to detect tier
4. Modify `get_token()` to use tier-based scoring
5. Unit tests and integration tests

**Estimated Effort**: 6 hours

---

## Related Documents

- [Subscription Tiers Reference](../antigravity/api/quota-apis.md#subscription-tiers)
- [Epic QUOTA-001](../epics/proactive-quota-monitoring-epic.md)

---

**Story History**:
- 2026-01-10: Story created
