# Story-015-02: Cache Monitoring Dashboard

**Epic**: Epic-015 - Gemini 2.5 Pro Thinking Optimization (Team 1, Gemini Specialists)
**Priority**: P2 (MEDIUM - operational visibility)
**Effort**: 3 days
**Assignee**: Dev 1B (Team 1 Mid-Level)
**Status**: ✅ COMPLETE - QA PASSED (10/10) - 28/28 Tests
**Created**: 2026-01-12

---

## Objective

Add comprehensive observability to the signature-based caching system implemented in Epic-013 Story 013-05, providing real-time metrics on cache hit rate, cost savings, cache size, and invalidation patterns. This dashboard will enable operators to validate cache effectiveness and optimize caching strategies.

---

## Business Context

### Problem Statement

The signature-based caching system (Epic-013 Story 013-05) currently operates as a "black box":
- **No visibility**: Cannot measure cache effectiveness (hit rate unknown)
- **No cost validation**: Cannot quantify API call savings from caching
- **No capacity planning**: Cache size and memory usage unmeasured
- **No optimization insights**: Invalidation patterns and TTL effectiveness unknown

### Success Metrics

**Primary KPI**: Cache hit rate visibility (percentage)
**Secondary KPI**: Cost savings calculation (USD saved via cache hits)
**Operational Target**: Dashboard loads <500ms
**Data Retention**: 7 days minimum historical data

### Business Value

- **Cost transparency**: Quantify ROI of caching infrastructure
- **Operational confidence**: Validate cache is working as designed
- **Optimization data**: Identify opportunities to improve cache strategy
- **Troubleshooting**: Diagnose cache-related issues quickly

---

## Acceptance Criteria

### AC1: Cache Hit Rate Metric Implemented

**GIVEN** requests processed through the proxy
**WHEN** cache is enabled for Gemini 2.5 Pro Thinking
**THEN** cache hit rate MUST be calculated and exposed

**Metric Definition**:
```yaml
Cache Hit Rate (%) = (Cache Hits / Total Cacheable Requests) × 100

Where:
  - Cache Hits: Requests served from cache (signature match found)
  - Total Cacheable Requests: All Gemini 2.5 Pro Thinking requests
  - Excludes: Streaming requests (not cached), requests with cache disabled
```

**Granularity**:
- **Real-time**: Current session hit rate (since app start)
- **Hourly**: Rolling 24-hour window (hourly buckets)
- **Daily**: Last 7 days (daily aggregates)

**Implementation**:
```rust
struct CacheMetrics {
    total_requests: u64,      // Total cacheable requests
    cache_hits: u64,          // Requests served from cache
    cache_misses: u64,        // Requests forwarded to API
    hit_rate_percent: f64,    // Calculated: hits / total × 100
}
```

---

### AC2: Cost Savings Calculated and Displayed

**GIVEN** cache hits avoiding upstream API calls
**WHEN** displaying cost metrics
**THEN** cost savings MUST be calculated in USD

**Cost Calculation**:
```yaml
Cost Savings (USD) = Cache Hits × Average Cost Per Request

Where:
  - Cache Hits: Number of requests served from cache
  - Average Cost Per Request: Calculated from actual API usage
    - Input tokens × $0.0525/1M
    - Thinking tokens × $0.35/1M
    - Output tokens × $0.21/1M

Example:
  - Cache Hits: 1000 requests
  - Avg Request Cost: $0.015 (calculated from token usage)
  - Savings: 1000 × $0.015 = $15.00 USD
```

**Display Requirements**:
- Real-time cumulative savings (since app start)
- Daily savings (last 7 days, bar chart)
- Monthly projection (estimated based on current rate)

---

### AC3: Cache Size Monitoring (Entries + Memory)

**GIVEN** cache storing responses
**WHEN** monitoring cache capacity
**THEN** size MUST be tracked in entries and memory usage

**Metrics**:
```yaml
Cache Size Metrics:
  - entry_count: Number of cached responses (integer)
  - memory_bytes: Estimated memory usage (bytes)
  - max_entries: Configured cache capacity limit
  - utilization_percent: (entry_count / max_entries) × 100
```

**Memory Estimation**:
```rust
fn estimate_cache_entry_size(entry: &CacheEntry) -> usize {
    // Approximate memory footprint
    let signature_size = entry.signature.len();
    let response_size = serde_json::to_string(&entry.response).unwrap().len();
    let metadata_size = 128; // Fixed overhead
    signature_size + response_size + metadata_size
}
```

**Alerts**:
- Warning at 80% capacity
- Critical at 95% capacity
- Automatic eviction follows LRU policy (already implemented in Epic-013)

---

### AC4: Dashboard Shows All Metrics

**GIVEN** cache monitoring data
**WHEN** user views API Proxy page
**THEN** dashboard MUST display all cache metrics

**Dashboard Layout** (Add to existing API Proxy page):
```
┌─────────────────────────────────────────────────────────┐
│ Cache Performance                                        │
├─────────────────────────────────────────────────────────┤
│ ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│ │ Hit Rate     │  │ Cost Savings │  │ Cache Size   │  │
│ │ 67.3%        │  │ $42.50       │  │ 156 / 500    │  │
│ │ ↑ +5.2%      │  │ Today: $8.20 │  │ 31.2% full   │  │
│ └──────────────┘  └──────────────┘  └──────────────┘  │
├─────────────────────────────────────────────────────────┤
│ Hit Rate Trend (7 Days)              Memory Usage      │
│ [Line Chart: Daily Hit Rate]         [Pie Chart]       │
│                                       - Cached: 4.2 MB  │
│                                       - Available: 9.8  │
├─────────────────────────────────────────────────────────┤
│ Invalidation Patterns (24h)                            │
│ [Bar Chart: TTL Expiry, Manual, Capacity]              │
└─────────────────────────────────────────────────────────┘
```

**UI Requirements**:
- Add "Cache Performance" section to API Proxy page
- Real-time updates (refresh every 5 seconds via Tauri command)
- Responsive design (mobile/tablet compatible)
- Export metrics as CSV (bonus feature)

---

### AC5: Historical Data (7 Days Minimum)

**GIVEN** cache operating over multiple days
**WHEN** viewing historical metrics
**THEN** data MUST be retained for at least 7 days

**Data Structure**:
```rust
struct HistoricalCacheMetrics {
    date: chrono::NaiveDate,
    total_requests: u64,
    cache_hits: u64,
    cache_misses: u64,
    hit_rate_percent: f64,
    cost_savings_usd: f64,
    avg_cache_size: f64,        // Average entries for the day
}
```

**Storage**:
- SQLite database: `cache_metrics_history` table
- Daily aggregation: Rollup at midnight UTC
- Retention: 7 days minimum, 30 days recommended
- Automatic cleanup: Delete records older than 30 days

**Schema**:
```sql
CREATE TABLE cache_metrics_history (
    date TEXT PRIMARY KEY,
    total_requests INTEGER NOT NULL,
    cache_hits INTEGER NOT NULL,
    cache_misses INTEGER NOT NULL,
    hit_rate_percent REAL NOT NULL,
    cost_savings_usd REAL NOT NULL,
    avg_cache_size REAL NOT NULL,
    created_at TEXT NOT NULL
);
```

---

### AC6: Comprehensive Test Coverage

**Unit Tests** (15+ tests minimum):
```yaml
Metrics Calculation:
  - test_hit_rate_calculation_basic
  - test_hit_rate_with_zero_requests
  - test_hit_rate_100_percent
  - test_cost_savings_calculation
  - test_cost_savings_with_variable_costs
  - test_cache_size_entry_count
  - test_cache_size_memory_estimation
  - test_cache_utilization_percent

Historical Data:
  - test_daily_aggregation
  - test_historical_data_retention_7_days
  - test_historical_data_cleanup_old_records
  - test_historical_query_by_date_range

Invalidation Tracking:
  - test_track_ttl_expiry_invalidation
  - test_track_manual_invalidation
  - test_track_capacity_eviction
```

**Integration Tests** (10+ tests minimum):
```yaml
End-to-End Metrics:
  - test_metrics_updated_after_cache_hit
  - test_metrics_updated_after_cache_miss
  - test_cost_savings_accumulation
  - test_cache_size_increases_after_miss
  - test_cache_size_decreases_after_eviction

Dashboard Integration:
  - test_tauri_command_get_cache_metrics
  - test_tauri_command_get_historical_metrics
  - test_metrics_refresh_every_5_seconds

Database Integration:
  - test_daily_aggregation_to_database
  - test_historical_query_returns_7_days
```

---

## Implementation Details

### Module Structure

```
src-tauri/src/proxy/
├── cache.rs                  (EXTEND from Epic-013)
│   ├── struct CacheMetrics   (NEW)
│   ├── impl Cache::get_metrics()
│   ├── impl Cache::record_hit()
│   ├── impl Cache::record_miss()
│   └── impl Cache::estimate_memory_usage()
├── monitor.rs                (EXTEND from Epic-013)
│   ├── struct CacheMonitor   (NEW)
│   ├── impl CacheMonitor::get_cache_metrics()
│   ├── impl CacheMonitor::get_historical_metrics()
│   └── impl CacheMonitor::aggregate_daily_metrics()
└── cache_db.rs               (NEW - 200 lines)
    ├── init_cache_metrics_table()
    ├── save_daily_metrics()
    ├── query_historical_metrics()
    └── cleanup_old_metrics()

src-tauri/src/commands/
└── proxy.rs                  (EXTEND)
    ├── get_cache_metrics()   (NEW Tauri command)
    └── get_cache_history()   (NEW Tauri command)

src/
├── pages/ApiProxy.tsx        (EXTEND)
│   └── Add CachePerformance section
├── components/proxy/
│   ├── CacheMetricsCard.tsx  (NEW)
│   ├── CacheHitRateChart.tsx (NEW)
│   └── CacheSizeIndicator.tsx (NEW)
└── services/
    └── cacheService.ts        (NEW)
        ├── getCacheMetrics()
        └── getCacheHistory()
```

### Data Structures

```rust
// src-tauri/src/proxy/cache.rs (EXTEND)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMetrics {
    /// Total cacheable requests processed
    pub total_requests: u64,

    /// Requests served from cache
    pub cache_hits: u64,

    /// Requests forwarded to API
    pub cache_misses: u64,

    /// Hit rate percentage (0.0-100.0)
    pub hit_rate_percent: f64,

    /// Estimated cost savings in USD
    pub cost_savings_usd: f64,

    /// Current cache size (entry count)
    pub entry_count: usize,

    /// Estimated memory usage in bytes
    pub memory_bytes: usize,

    /// Configured maximum entries
    pub max_entries: usize,

    /// Cache utilization percentage (0.0-100.0)
    pub utilization_percent: f64,

    /// Invalidation counts by type
    pub invalidations: InvalidationCounts,

    /// Last updated timestamp
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidationCounts {
    pub ttl_expiry: u64,
    pub manual_clear: u64,
    pub capacity_eviction: u64,
}

impl Cache {
    pub fn get_metrics(&self) -> CacheMetrics {
        let guard = self.entries.lock().unwrap();

        let entry_count = guard.len();
        let memory_bytes = self.estimate_memory_usage(&guard);
        let utilization_percent = if self.max_entries > 0 {
            (entry_count as f64 / self.max_entries as f64) * 100.0
        } else {
            0.0
        };

        let hit_rate_percent = if self.total_requests > 0 {
            (self.cache_hits as f64 / self.total_requests as f64) * 100.0
        } else {
            0.0
        };

        CacheMetrics {
            total_requests: self.total_requests,
            cache_hits: self.cache_hits,
            cache_misses: self.cache_misses,
            hit_rate_percent,
            cost_savings_usd: self.calculate_cost_savings(),
            entry_count,
            memory_bytes,
            max_entries: self.max_entries,
            utilization_percent,
            invalidations: self.invalidations.clone(),
            last_updated: chrono::Utc::now(),
        }
    }

    pub fn record_hit(&mut self) {
        self.cache_hits += 1;
        self.total_requests += 1;
    }

    pub fn record_miss(&mut self) {
        self.cache_misses += 1;
        self.total_requests += 1;
    }

    fn estimate_memory_usage(&self, entries: &HashMap<String, CacheEntry>) -> usize {
        entries.iter()
            .map(|(key, entry)| {
                let key_size = key.len();
                let value_size = serde_json::to_string(&entry.response)
                    .map(|s| s.len())
                    .unwrap_or(0);
                key_size + value_size + 128 // 128 bytes metadata overhead
            })
            .sum()
    }

    fn calculate_cost_savings(&self) -> f64 {
        // Average cost per request (from historical data)
        let avg_cost_per_request = 0.015; // $0.015 per request (typical)
        self.cache_hits as f64 * avg_cost_per_request
    }
}

// src-tauri/src/proxy/cache_db.rs (NEW)

use rusqlite::{Connection, Result};

pub fn init_cache_metrics_table(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS cache_metrics_history (
            date TEXT PRIMARY KEY,
            total_requests INTEGER NOT NULL,
            cache_hits INTEGER NOT NULL,
            cache_misses INTEGER NOT NULL,
            hit_rate_percent REAL NOT NULL,
            cost_savings_usd REAL NOT NULL,
            avg_cache_size REAL NOT NULL,
            created_at TEXT NOT NULL
        )",
        [],
    )?;
    Ok(())
}

pub fn save_daily_metrics(
    conn: &Connection,
    metrics: &HistoricalCacheMetrics,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO cache_metrics_history
         (date, total_requests, cache_hits, cache_misses, hit_rate_percent,
          cost_savings_usd, avg_cache_size, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            metrics.date.to_string(),
            metrics.total_requests,
            metrics.cache_hits,
            metrics.cache_misses,
            metrics.hit_rate_percent,
            metrics.cost_savings_usd,
            metrics.avg_cache_size,
            chrono::Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn query_historical_metrics(
    conn: &Connection,
    days: i64,
) -> Result<Vec<HistoricalCacheMetrics>> {
    let cutoff_date = chrono::Utc::now().naive_utc().date() - chrono::Duration::days(days);

    let mut stmt = conn.prepare(
        "SELECT date, total_requests, cache_hits, cache_misses,
                hit_rate_percent, cost_savings_usd, avg_cache_size
         FROM cache_metrics_history
         WHERE date >= ?1
         ORDER BY date DESC"
    )?;

    let metrics = stmt.query_map([cutoff_date.to_string()], |row| {
        Ok(HistoricalCacheMetrics {
            date: chrono::NaiveDate::parse_from_str(&row.get::<_, String>(0)?, "%Y-%m-%d")
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?,
            total_requests: row.get(1)?,
            cache_hits: row.get(2)?,
            cache_misses: row.get(3)?,
            hit_rate_percent: row.get(4)?,
            cost_savings_usd: row.get(5)?,
            avg_cache_size: row.get(6)?,
        })
    })?;

    metrics.collect()
}

pub fn cleanup_old_metrics(conn: &Connection, days: i64) -> Result<usize> {
    let cutoff_date = chrono::Utc::now().naive_utc().date() - chrono::Duration::days(days);

    conn.execute(
        "DELETE FROM cache_metrics_history WHERE date < ?1",
        [cutoff_date.to_string()],
    )
}
```

### Tauri Commands

```rust
// src-tauri/src/commands/proxy.rs (EXTEND)

#[tauri::command]
pub async fn get_cache_metrics(
    state: tauri::State<'_, AppState>,
) -> Result<CacheMetrics, String> {
    let monitor = state.proxy_monitor.lock().await;

    if let Some(cache) = &monitor.cache {
        Ok(cache.get_metrics())
    } else {
        Err("Cache not initialized".to_string())
    }
}

#[tauri::command]
pub async fn get_cache_history(
    state: tauri::State<'_, AppState>,
    days: Option<i64>,
) -> Result<Vec<HistoricalCacheMetrics>, String> {
    let days = days.unwrap_or(7);
    let db = state.db.lock().await;

    cache_db::query_historical_metrics(&db, days)
        .map_err(|e| format!("Failed to query cache history: {}", e))
}
```

### Frontend Implementation

```typescript
// src/services/cacheService.ts (NEW)

import { invoke } from '@tauri-apps/api/core';

export interface CacheMetrics {
  total_requests: number;
  cache_hits: number;
  cache_misses: number;
  hit_rate_percent: number;
  cost_savings_usd: number;
  entry_count: number;
  memory_bytes: number;
  max_entries: number;
  utilization_percent: number;
  invalidations: {
    ttl_expiry: number;
    manual_clear: number;
    capacity_eviction: number;
  };
  last_updated: string;
}

export interface HistoricalCacheMetrics {
  date: string;
  total_requests: number;
  cache_hits: number;
  cache_misses: number;
  hit_rate_percent: number;
  cost_savings_usd: number;
  avg_cache_size: number;
}

export async function getCacheMetrics(): Promise<CacheMetrics> {
  return invoke('get_cache_metrics');
}

export async function getCacheHistory(days?: number): Promise<HistoricalCacheMetrics[]> {
  return invoke('get_cache_history', { days });
}
```

```tsx
// src/components/proxy/CacheMetricsCard.tsx (NEW)

import React, { useEffect, useState } from 'react';
import { getCacheMetrics, CacheMetrics } from '@/services/cacheService';

export const CacheMetricsCard: React.FC = () => {
  const [metrics, setMetrics] = useState<CacheMetrics | null>(null);

  useEffect(() => {
    const fetchMetrics = async () => {
      try {
        const data = await getCacheMetrics();
        setMetrics(data);
      } catch (err) {
        console.error('Failed to fetch cache metrics:', err);
      }
    };

    fetchMetrics();
    const interval = setInterval(fetchMetrics, 5000); // Refresh every 5s

    return () => clearInterval(interval);
  }, []);

  if (!metrics) return <div>Loading cache metrics...</div>;

  return (
    <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
      <div className="stat bg-base-200 rounded-lg p-4">
        <div className="stat-title">Hit Rate</div>
        <div className="stat-value text-success">{metrics.hit_rate_percent.toFixed(1)}%</div>
        <div className="stat-desc">
          {metrics.cache_hits} hits / {metrics.total_requests} requests
        </div>
      </div>

      <div className="stat bg-base-200 rounded-lg p-4">
        <div className="stat-title">Cost Savings</div>
        <div className="stat-value text-primary">${metrics.cost_savings_usd.toFixed(2)}</div>
        <div className="stat-desc">
          {metrics.cache_hits} API calls avoided
        </div>
      </div>

      <div className="stat bg-base-200 rounded-lg p-4">
        <div className="stat-title">Cache Size</div>
        <div className="stat-value">{metrics.entry_count} / {metrics.max_entries}</div>
        <div className="stat-desc">
          {(metrics.memory_bytes / 1024 / 1024).toFixed(1)} MB ({metrics.utilization_percent.toFixed(1)}%)
        </div>
      </div>
    </div>
  );
};
```

---

## Test Strategy

### Phase 1: Unit Testing (Day 1)
**Focus**: Metrics calculation logic

```bash
cargo test --package antigravity_tools_lib cache::metrics
```

**Key Tests**:
- Hit rate calculation (various scenarios)
- Cost savings calculation
- Memory estimation
- Historical aggregation

---

### Phase 2: Integration Testing (Day 2)
**Focus**: Database integration, Tauri commands

```bash
cargo test --package antigravity_tools_lib cache::integration
```

**Key Tests**:
- Daily aggregation to database
- Historical query (7 days)
- Tauri command integration
- Metrics refresh flow

---

### Phase 3: UI Testing (Day 3)
**Focus**: Dashboard rendering, real-time updates

**Manual Testing Checklist**:
- ✅ Cache metrics cards render correctly
- ✅ Metrics update every 5 seconds
- ✅ Hit rate chart shows 7-day trend
- ✅ Cost savings calculation accurate
- ✅ Cache size indicator responsive

---

## Dependencies

### Internal Dependencies
- **Epic-013 Story 013-05**: Cache module (`src-tauri/src/proxy/cache.rs`) - REQUIRED
- **Epic-013 Story 013-06**: Monitor module (`src-tauri/src/proxy/monitor.rs`) - REQUIRED
- **SQLite database**: Already initialized in app - STABLE

### External Dependencies
- **Chart library** (Frontend): Consider Recharts or Chart.js for visualizations
- **No new Rust crates required**

---

## Success Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| Dashboard load time | <500ms | Browser DevTools |
| Metrics refresh latency | <100ms | Tauri command timing |
| Data retention | 7+ days | Database query validation |
| UI responsiveness | 60 fps | Visual inspection |
| Test coverage | 85%+ | `cargo tarpaulin` |

---

## Definition of Done

### Code Complete
- ✅ Cache metrics tracking in `cache.rs`
- ✅ Historical database storage in `cache_db.rs`
- ✅ Tauri commands for metrics retrieval
- ✅ Frontend dashboard components

### Testing Complete
- ✅ 15+ unit tests passing
- ✅ 10+ integration tests passing
- ✅ Manual UI testing checklist complete

### Documentation Complete
- ✅ Inline code documentation
- ✅ Dashboard usage guide
- ✅ Metrics interpretation guide

### Deployment Ready
- ✅ No breaking changes
- ✅ Database migration handled
- ✅ Dashboard integrated into API Proxy page

---

## Risk Assessment

**Risk 1**: Database migration issues
- **Impact**: MEDIUM
- **Probability**: LOW
- **Mitigation**: Use `IF NOT EXISTS` in schema creation

**Risk 2**: Performance impact of metrics tracking
- **Impact**: LOW
- **Probability**: LOW
- **Mitigation**: Lightweight calculations, async aggregation

---

## Future Enhancements

- Export metrics as CSV
- Cache warming strategies
- Predictive cache capacity alerts
- Per-account cache metrics

---

## Notes

- Builds directly on Epic-013 cache foundation
- No conflicts with Team 2 work
- Enables data-driven cache optimization
