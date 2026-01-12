# Story-013-05: Caching Integration

**Epic**: Epic-013 (Gemini 3 Flash Optimization)
**Дата создания**: 2026-01-12
**Статус**: 📋 OPTIONAL (P3 - Future Enhancement)
**Приоритет**: P3 (Low - Not Critical for Compliance)
**Оценка**: 2-3 дня

---

## 🎯 User Story

**As a** System Administrator
**I want** signature-based caching for Gemini API responses
**So that** we reduce API costs and improve response times for repeated queries

**Context**: This is an **optimization enhancement**, NOT required for 95% compliance target. Consider deferring to Q2 2026 after Epic-013 core objectives are met.

---

## 📋 Background

### Current State

**No Caching**: Every request hits Google API, even for identical queries

```yaml
current_behavior:
  repeated_query: "What is 2+2?"
  request_1: "Hits Google API, pays cost, ~500ms latency"
  request_2_identical: "Hits Google API again, pays cost again, ~500ms latency"
  request_3_identical: "Hits Google API again..."

cost_impact:
  - "Pay for every request, even duplicates"
  - "Higher latency (network round-trip each time)"
  - "No benefit from repetition"
```

### Proposed Solution

**Signature-Based Caching**:

```yaml
caching_strategy:
  cache_key:
    components:
      - "model name (e.g., gemini-3-flash)"
      - "prompt hash (SHA-256 of messages)"
      - "thinking level (LOW/MEDIUM/HIGH)"
      - "temperature, top_p, max_tokens"
    example: "gemini-3-flash:abc123def:MEDIUM:0.7:0.9:1000"

  cache_hit:
    behavior: "Return cached response immediately"
    cost: "$0 (no API call)"
    latency: "<50ms (in-memory retrieval)"

  cache_miss:
    behavior: "Call Google API, cache response"
    cost: "Normal API cost"
    latency: "Normal API latency (~500ms)"

  ttl:
    default: "1 hour"
    configurable: "5 minutes to 24 hours"
    thinking_responses: "1 hour (default)"
    simple_responses: "24 hours (optional longer TTL)"
```

### Gap Reference

**From**: `gemini-3-flash-COMPARISON.md` (NOT listed as gap)

**Note**: Caching is an **architectural enhancement**, not a documented gap in COMPARISON.md. It's a **performance optimization**, not a compliance requirement.

---

## 🔧 Technical Details

### Cache Architecture

**Storage Options**:

1. **In-Memory Cache** (Recommended for MVP)
   - **Technology**: `lru-cache` crate (Rust)
   - **Capacity**: 1000 entries (configurable)
   - **Eviction**: LRU (Least Recently Used)
   - **Pros**: Simple, fast (<1ms lookup)
   - **Cons**: Lost on restart, single-instance only

2. **Redis Cache** (Future Enhancement)
   - **Technology**: Redis cluster
   - **Capacity**: Limited by Redis memory
   - **Eviction**: LRU or TTL-based
   - **Pros**: Persistent, multi-instance shared cache
   - **Cons**: Network latency, infrastructure dependency

**Recommendation**: Start with **In-Memory** (MVP), add Redis later if needed

### Cache Key Generation

```rust
use sha2::{Sha256, Digest};

fn generate_cache_key(request: &GeminiRequest) -> String {
    let mut hasher = Sha256::new();

    // Hash components
    hasher.update(request.model.as_bytes());
    hasher.update(serde_json::to_string(&request.messages).unwrap().as_bytes());

    // Include thinking config if present
    if let Some(thinking) = &request.thinking_config {
        hasher.update(thinking.level.as_bytes());
    }

    // Include generation params
    hasher.update(request.temperature.to_string().as_bytes());
    hasher.update(request.top_p.to_string().as_bytes());
    hasher.update(request.max_tokens.to_string().as_bytes());

    // Compute hash
    let hash = hasher.finalize();
    format!("gemini:{}:{:x}", request.model, hash)
}
```

### Cache Implementation

**File**: `src-tauri/src/proxy/cache/mod.rs` (new module)

```rust
use lru::LruCache;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct ResponseCache {
    cache: Arc<Mutex<LruCache<String, CachedResponse>>>,
    ttl: Duration,
}

struct CachedResponse {
    response: Value,
    cached_at: Instant,
}

impl ResponseCache {
    pub fn new(capacity: usize, ttl_seconds: u64) -> Self {
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(capacity))),
            ttl: Duration::from_secs(ttl_seconds),
        }
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        let mut cache = self.cache.lock().unwrap();

        if let Some(cached) = cache.get(key) {
            // Check TTL
            if cached.cached_at.elapsed() < self.ttl {
                return Some(cached.response.clone());
            } else {
                // Expired, remove
                cache.pop(key);
            }
        }

        None
    }

    pub fn put(&self, key: String, response: Value) {
        let mut cache = self.cache.lock().unwrap();
        cache.put(key, CachedResponse {
            response,
            cached_at: Instant::now(),
        });
    }
}
```

### Integration into Request Flow

**File**: `src-tauri/src/proxy/handlers/openai.rs` (or similar)

```rust
use crate::proxy::cache::ResponseCache;

lazy_static! {
    static ref RESPONSE_CACHE: ResponseCache = ResponseCache::new(1000, 3600);
}

pub async fn handle_openai_request(request: OpenAIRequest) -> Result<Value, Error> {
    // 1. Generate cache key
    let cache_key = generate_cache_key(&request);

    // 2. Check cache
    if let Some(cached_response) = RESPONSE_CACHE.get(&cache_key) {
        info!(
            category = "cache",
            cache_hit = true,
            cache_key = %cache_key,
            "Returning cached response"
        );
        return Ok(cached_response);
    }

    // 3. Cache miss - call Google API
    info!(
        category = "cache",
        cache_hit = false,
        cache_key = %cache_key,
        "Cache miss, calling Google API"
    );

    let response = call_google_api(&request).await?;

    // 4. Cache the response
    RESPONSE_CACHE.put(cache_key, response.clone());

    Ok(response)
}
```

---

## ✅ Acceptance Criteria

### AC-1: Cache Hit Performance

```gherkin
GIVEN a request has been processed and cached
WHEN an identical request arrives
THEN the response is returned from cache in <50ms
```

**Verification**:
- ✅ Cache hit latency <50ms (vs ~500ms API call)
- ✅ No Google API call made (verified in logs)
- ✅ Response identical to original

---

### AC-2: Cache Key Uniqueness

```gherkin
GIVEN two requests with different parameters
WHEN cache keys are generated
THEN the keys are different
```

**Test Matrix**:
| Request 1 | Request 2 | Keys Should Be |
|-----------|-----------|----------------|
| Flash, "Hi", MEDIUM | Flash, "Hi", HIGH | DIFFERENT ✅ |
| Flash, "Hi", MEDIUM | Pro, "Hi", MEDIUM | DIFFERENT ✅ |
| Flash, "Hi", MEDIUM | Flash, "Hello", MEDIUM | DIFFERENT ✅ |
| Flash, "Hi", MEDIUM | Flash, "Hi", MEDIUM | SAME ✅ |

**Verification**:
- ✅ Different models → different keys
- ✅ Different prompts → different keys
- ✅ Different levels → different keys
- ✅ Identical requests → same key

---

### AC-3: TTL Expiration

```gherkin
GIVEN a response cached with TTL = 1 hour
WHEN 61 minutes pass
THEN the cached entry is expired and removed
```

**Verification**:
- ✅ Fresh cache (<1 hour) returns hits
- ✅ Expired cache (>1 hour) returns miss
- ✅ Expired entries removed from memory

---

### AC-4: Configuration

```gherkin
GIVEN cache configuration options
WHEN system starts
THEN cache is configured correctly
```

**Config File** (`config.json`):
```json
{
  "cache": {
    "enabled": true,
    "capacity": 1000,
    "ttl_seconds": 3600,
    "models": {
      "gemini-3-flash": {
        "ttl_seconds": 1800
      },
      "gemini-3-pro-high": {
        "ttl_seconds": 7200
      }
    }
  }
}
```

**Verification**:
- ✅ Cache can be disabled via config
- ✅ TTL is configurable per model
- ✅ Capacity is configurable

---

### AC-5: Metrics and Monitoring

```gherkin
GIVEN caching is enabled
WHEN monitoring the system
THEN cache metrics are available
```

**Metrics to Track**:
```yaml
cache_metrics:
  hit_rate: "Percentage of requests served from cache"
  miss_rate: "Percentage requiring API calls"
  cache_size: "Current number of cached entries"
  evictions: "Number of LRU evictions"
  ttl_expirations: "Number of TTL-based removals"
```

**Verification**:
- ✅ Metrics logged to monitoring system
- ✅ Hit rate >20% in normal usage
- ✅ Cache size stays within limits

---

## 🔍 Implementation Guide

### Step 1: Add Dependencies

**File**: `src-tauri/Cargo.toml`

```toml
[dependencies]
lru = "0.12"
sha2 = "0.10"
lazy_static = "1.4"
```

### Step 2: Create Cache Module

**File**: `src-tauri/src/proxy/cache/mod.rs`

Implement:
- `ResponseCache` struct
- `get()` method (with TTL check)
- `put()` method
- `clear()` method (for testing/reset)

### Step 3: Integrate into Handlers

**File**: `src-tauri/src/proxy/handlers/openai.rs`

Modify:
1. Add cache check before Google API call
2. Store response in cache after API call
3. Log cache hits/misses

### Step 4: Add Configuration

**File**: `src-tauri/src/modules/config.rs`

Add cache config:
```rust
#[derive(Serialize, Deserialize)]
pub struct CacheConfig {
    pub enabled: bool,
    pub capacity: usize,
    pub ttl_seconds: u64,
}
```

### Step 5: Testing

**Unit Tests**:
```rust
#[test]
fn test_cache_hit() {
    let cache = ResponseCache::new(10, 3600);
    let key = "test_key".to_string();
    let value = json!({"result": "test"});

    cache.put(key.clone(), value.clone());
    let retrieved = cache.get(&key);

    assert_eq!(retrieved, Some(value));
}

#[test]
fn test_cache_ttl_expiration() {
    let cache = ResponseCache::new(10, 1); // 1 second TTL
    let key = "test_key".to_string();
    let value = json!({"result": "test"});

    cache.put(key.clone(), value.clone());
    std::thread::sleep(std::time::Duration::from_secs(2));

    let retrieved = cache.get(&key);
    assert_eq!(retrieved, None); // Expired
}
```

**Integration Tests**:
- Send identical requests 2x, verify 2nd is cached
- Verify different prompts don't hit same cache
- Verify TTL expiration works

---

## 📊 Quality Gates

### QG-1: Performance Improvement

**Benchmark**:
```bash
# Without cache: ~500ms average
# With cache (hit): ~50ms average
# Improvement: 10x faster
```

**Expected**:
- ✅ Cache hit latency <50ms
- ✅ Cache overhead (miss) <5ms

---

### QG-2: Memory Usage

**Monitor**:
```bash
# Cache size: 1000 entries * ~5KB each = ~5MB max
```

**Expected**:
- ✅ Memory usage <10MB for cache
- ✅ No memory leaks
- ✅ LRU eviction works

---

### QG-3: Cache Hit Rate

**Target**: >20% hit rate in normal usage

**Measurement**:
```yaml
cache_analytics:
  period: "7 days"
  total_requests: 10000
  cache_hits: 2500
  hit_rate: "25%"  # ✅ Exceeds 20% target
```

---

### QG-4: Correctness

**Verification**:
- ✅ Cached responses identical to fresh responses
- ✅ No stale data issues
- ✅ Proper key generation (unique per request variant)

---

## 🎯 Success Metrics

```yaml
performance:
  cache_hit_latency: "<50ms (vs 500ms API call)"
  improvement: "10x faster for cached requests"

cost_savings:
  api_cost_reduction: "Proportional to hit rate"
  example: "20% hit rate = 20% cost savings"

user_experience:
  faster_responses: "Instant for repeated queries"
  reduced_api_failures: "Cached responses don't fail"
```

---

## ⚠️ Important Notes

### Why This is P3 (Optional)

```yaml
priority_justification:
  compliance_impact: "ZERO - not required for 95% target"
  business_value: "Optimization, not core functionality"
  deferrable: "Can be added later without risk"

recommendation:
  epic_013_focus: "Core compliance gaps (Stories 013-01, 013-04, 013-06)"
  caching_timeline: "Q2 2026 or later"
  condition: "Only if excess capacity after core stories"
```

### Risks and Considerations

```yaml
risks:
  stale_data:
    risk: "Cached responses may become outdated"
    mitigation: "Short TTL (1 hour default)"

  cache_poisoning:
    risk: "Invalid response cached, served repeatedly"
    mitigation: "Validate responses before caching"

  memory_usage:
    risk: "Cache grows too large"
    mitigation: "LRU eviction, configurable capacity"

  complexity:
    risk: "Adds system complexity"
    mitigation: "Simple MVP (in-memory), defer Redis"
```

### Future Enhancements (Not in This Story)

- **Redis Integration**: Shared cache across instances
- **Cache Warming**: Pre-populate common queries
- **Smart TTL**: Adaptive TTL based on query patterns
- **Cache Analytics**: Detailed hit rate analysis

---

## 🔗 Related Work

**Dependencies**:
- ✅ NONE - Can be implemented independently

**Follow-up Stories**:
- Future: Redis cache backend (multi-instance)
- Future: Cache analytics dashboard

**References**:
- `lru` crate: https://docs.rs/lru/
- Caching best practices

---

## 📝 Notes

**Product Owner Decision Required**:

```yaml
decision_question: "Should Story-013-05 be included in Epic-013?"

option_1_include:
  timeline: "+2-3 days to Epic-013"
  benefit: "Performance optimization"
  risk: "Delays core compliance work"

option_2_defer:
  timeline: "Epic-013 completes 1-1.5 weeks faster"
  defer_to: "Q2 2026 after Strategic Review"
  benefit: "Focus on compliance first"

recommendation: "DEFER to Q2 2026" ⚠️
```

---

**Story Owner**: Backend Developer
**Reviewers**: Tech Lead, Architect
**Estimated Effort**: 2-3 days (16-24 hours)
**Actual Effort**: _TBD after completion_

**Status**: 📋 OPTIONAL (P3 - Consider Deferring)
**Next Step**: Product Owner decision: Include or Defer to Q2?
