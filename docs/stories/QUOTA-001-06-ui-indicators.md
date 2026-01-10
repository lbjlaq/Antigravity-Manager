# Story QUOTA-001-06: Quota Health UI Indicators

**Story ID**: QUOTA-001-06
**Epic**: [QUOTA-001](../epics/proactive-quota-monitoring-epic.md)
**Priority**: P2
**Estimate**: 3 points
**Status**: To Do
**Dependencies**: QUOTA-001-03

---

## User Story

**As a** user
**I want** to see quota health status in the account management UI
**So that** I'm aware of quota consumption and upcoming resets

---

## Acceptance Criteria

### AC-1: Quota Display

- [ ] Show `remainingFraction` percentage per account
- [ ] Color-coded indicators:
  - Green: >20%
  - Yellow: 10-20%
  - Red: <10%
  - Gray: No data
- [ ] Display `resetTime` in local timezone

### AC-2: Real-Time Updates

- [ ] Updates from background monitoring (every 5 min)
- [ ] Updates on 429 errors (immediate)
- [ ] Tauri event emission for UI updates

**Event Structure**:
```rust
app.emit("quota://updated", QuotaUpdateEvent {
    account_id: String,
    model: String,
    remaining_fraction: f64,
    reset_time: String,
});
```

---

## Implementation

**Files**:
- `src/pages/Accounts.tsx` - Account list UI
- `src/components/accounts/QuotaIndicator.tsx` - New component
- `src-tauri/src/proxy/quota_manager.rs` - Event emission

**Estimated Effort**: 5 hours

---

## Related Documents

- [Epic QUOTA-001](../epics/proactive-quota-monitoring-epic.md)

---

**Story History**:
- 2026-01-10: Story created
