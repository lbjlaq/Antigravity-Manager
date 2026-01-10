# Story QUOTA-001-03: Background Quota Monitoring

**Story ID**: QUOTA-001-03
**Epic**: [QUOTA-001 - Proactive Quota Monitoring](../epics/proactive-quota-monitoring-epic.md)
**Priority**: P1 (High)
**Estimate**: 5 story points
**Status**: To Do
**Dependencies**: QUOTA-001-01, QUOTA-001-05

---

## User Story

**As a** system
**I want** to periodically refresh quota information in the background
**So that** quota data remains up-to-date without blocking API requests

---

## Context

Quota cache needs regular refreshing to stay accurate. A background Tokio task should fetch quotas every 5 minutes for all accounts.

**Reference**: [quota-apis.md#aggregate-tracking](../antigravity/api/quota-apis.md#aggregate-tracking)

---

## Acceptance Criteria

### AC-1: Background Task Implementation

- [ ] Tokio task spawned on proxy startup
- [ ] Runs every 5 minutes (configurable)
- [ ] Non-blocking execution (doesn't interfere with requests)
- [ ] Graceful shutdown on proxy stop

**Implementation**:
```rust
pub async fn start_background_monitoring(&self) {
    let tokens = Arc::clone(&self.tokens);
    let cache = Arc::clone(&self.quota_manager.cache);
    let fetcher = self.quota_manager.fetcher.clone();

    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 min

        loop {
            interval.tick().await;

            tracing::info!("[Quota-Monitor] Starting quota refresh for all accounts");
            let start = std::time::Instant::now();

            for entry in tokens.iter() {
                let token = entry.value();

                match fetcher.fetch_available_models(&token.access_token, &token.project_id).await {
                    Ok(quotas) => {
                        for (model, quota_info) in quotas {
                            let key = format!("{}:{}", token.account_id, model);
                            cache.insert(key, CachedQuotaInfo {
                                remaining_fraction: quota_info.remaining_fraction,
                                reset_time: quota_info.to_system_time().unwrap(),
                                fetched_at: SystemTime::now(),
                                ttl: Duration::from_secs(300),
                            });
                        }
                        tracing::debug!("✅ Refreshed quota for {}", token.account_id);
                    }
                    Err(e) => {
                        tracing::warn!("❌ Failed to refresh quota for {}: {}", token.account_id, e);
                    }
                }
            }

            let elapsed = start.elapsed();
            tracing::info!("[Quota-Monitor] Refresh completed in {:?}", elapsed);
        }
    });

    self.monitor_task = Some(task);
}
```

---

### AC-2: Error Handling

- [ ] Handle 401 (log, mark account for token refresh)
- [ ] Handle 403 (log, mark account as forbidden)
- [ ] Handle 429 (exponential backoff, skip this refresh)
- [ ] Handle 500/503 (retry once, then skip)
- [ ] Continue monitoring even if some accounts fail

---

### AC-3: Configuration

- [ ] Configurable refresh interval (default: 5 minutes)
- [ ] Configurable in `config.json`
- [ ] Valid range: 1-60 minutes

**Config Structure**:
```json
{
  "proxy_config": {
    "quota_monitoring": {
      "enabled": true,
      "refresh_interval_seconds": 300,
      "cache_ttl_seconds": 300
    }
  }
}
```

---

### AC-4: Graceful Shutdown

- [ ] Task cleanly terminates on proxy shutdown
- [ ] No zombie tasks after restart
- [ ] Handle SIGTERM/SIGINT properly

---

## Implementation Tasks

### Task 1: Background Task Function

**Estimated Effort**: 2 hours

### Task 2: Configuration Loading

**Estimated Effort**: 1 hour

### Task 3: Error Handling

**Estimated Effort**: 1.5 hours

### Task 4: Testing (48-hour stability test)

**Estimated Effort**: 2 hours setup + monitoring

---

## Testing Strategy

### Stability Test

**Duration**: 48 hours continuous operation

**Metrics**:
- Task uptime: 100%
- Successful refresh rate: >95%
- Memory leaks: None
- CPU usage: <1%

---

## Related Documents

- [Epic QUOTA-001](../epics/proactive-quota-monitoring-epic.md)
- [QUOTA-001-01: API Integration](QUOTA-001-01-quota-api-integration.md)
- [Quota APIs Reference](../antigravity/api/quota-apis.md)

---

**Story History**:
- 2026-01-10: Story created
