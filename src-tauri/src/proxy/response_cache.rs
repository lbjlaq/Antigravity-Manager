//! Response caching for Gemini thinking API responses (Story-013-05)
//!
//! Provides signature-based response caching to reduce API costs and improve latency.
//! This is SEPARATE from the image cache (cache.rs) which handles Imagen responses.
//!
//! # Architecture
//!
//! - **LRU eviction**: Least-recently-used entries removed when capacity exceeded
//! - **TTL expiration**: Configurable time-to-live for cached entries
//! - **Thread-safe**: Arc<Mutex<LruCache>> for concurrent access
//! - **Cache key format**: `gemini:{model}:{thinking_level}:{temp}:{top_p}:{max_tokens}:{prompt_hash}`
//!
//! # Performance Targets
//!
//! - Cache hit: <50ms (vs ~500ms API call)
//! - Cache miss overhead: <10ms
//! - Cache hit rate: ≥20% in production
//! - Memory usage: <10MB for 1000 entries
//!
//! # Configuration
//!
//! ```rust
//! ResponseCacheConfig {
//!     enabled: true,
//!     capacity: 1000,
//!     ttl_seconds: 3600, // 1 hour
//! }
//! ```

use lru::LruCache;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, info};

// ==================================================================================
// DATA STRUCTURES
// ==================================================================================

/// Cached response entry with metadata
#[derive(Debug, Clone)]
pub struct CachedResponse {
    /// Cached JSON response
    pub response: Value,
    /// Unix timestamp when cached
    pub cached_at: u64,
    /// Unix timestamp when entry expires
    pub expires_at: u64,
}

/// Cache statistics for monitoring
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses
    pub misses: u64,
    /// Number of evictions (LRU)
    pub evictions: u64,
    /// Total number of entries
    pub entry_count: u64,
}

impl CacheStats {
    /// Calculate cache hit rate (0.0 - 1.0)
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Calculate cache miss rate (0.0 - 1.0)
    pub fn miss_rate(&self) -> f64 {
        1.0 - self.hit_rate()
    }
}

// ==================================================================================
// RESPONSE CACHE
// ==================================================================================

/// Thread-safe LRU cache for API responses
///
/// Uses Arc<Mutex<LruCache>> for safe concurrent access from multiple threads.
pub struct ResponseCache {
    /// Inner LRU cache (thread-safe)
    cache: Arc<Mutex<LruCache<String, CachedResponse>>>,
    /// Default TTL for new entries
    ttl: Duration,
    /// Cache statistics (thread-safe)
    stats: Arc<Mutex<CacheStats>>,
}

impl ResponseCache {
    /// Create new response cache
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of entries (LRU eviction when exceeded)
    /// * `ttl_seconds` - Default time-to-live for cached entries
    ///
    /// # Example
    ///
    /// ```rust
    /// let cache = ResponseCache::new(1000, 3600); // 1000 entries, 1 hour TTL
    /// ```
    pub fn new(capacity: usize, ttl_seconds: u64) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(1000).unwrap());
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(cap))),
            ttl: Duration::from_secs(ttl_seconds),
            stats: Arc::new(Mutex::new(CacheStats::default())),
        }
    }

    /// Lookup cached response by key
    ///
    /// Returns `Some(Value)` on cache hit, `None` on miss or expiration.
    /// Expired entries are automatically removed.
    ///
    /// # Performance
    /// Target: <50ms (p99)
    pub fn get(&self, key: &str) -> Option<Value> {
        let mut cache = self.cache.lock().unwrap();
        let mut stats = self.stats.lock().unwrap();

        if let Some(cached) = cache.get(key) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            // Check expiration
            if now > cached.expires_at {
                // Expired - remove and count as miss
                cache.pop(key);
                stats.misses += 1;
                stats.entry_count = stats.entry_count.saturating_sub(1);
                debug!(
                    category = "cache",
                    cache_hit = false,
                    cache_key = %key,
                    reason = "expired",
                    "Cache miss: entry expired"
                );
                return None;
            }

            // Valid hit
            stats.hits += 1;
            info!(
                category = "cache",
                cache_hit = true,
                cache_key = %key,
                age_seconds = now - cached.cached_at,
                "Cache hit: returning cached response"
            );
            return Some(cached.response.clone());
        }

        // Cache miss
        stats.misses += 1;
        debug!(
            category = "cache",
            cache_hit = false,
            cache_key = %key,
            reason = "not_found",
            "Cache miss: key not found"
        );
        None
    }

    /// Store response in cache with TTL
    ///
    /// Evicts oldest entry if capacity exceeded (LRU).
    ///
    /// # Arguments
    ///
    /// * `key` - Cache key (generated by `generate_cache_key`)
    /// * `response` - JSON response to cache
    pub fn put(&self, key: String, response: Value) {
        let mut cache = self.cache.lock().unwrap();
        let mut stats = self.stats.lock().unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expires_at = now + self.ttl.as_secs();

        // Check if we need to evict (LRU handles this automatically, but track stats)
        let will_evict = cache.len() >= cache.cap().get() && !cache.contains(&key);
        if will_evict {
            stats.evictions += 1;
        }

        // Insert into cache (LRU handles eviction automatically)
        let is_new = !cache.contains(&key);
        cache.put(
            key.clone(),
            CachedResponse {
                response,
                cached_at: now,
                expires_at,
            },
        );

        // Update stats
        if is_new {
            stats.entry_count += 1;
        }

        debug!(
            category = "cache",
            cache_key = %key,
            ttl_seconds = self.ttl.as_secs(),
            evicted = will_evict,
            "Response cached successfully"
        );
    }

    /// Delete specific cache entry
    ///
    /// Used for manual cache invalidation.
    #[allow(dead_code)]
    pub fn delete(&self, key: &str) {
        let mut cache = self.cache.lock().unwrap();
        let mut stats = self.stats.lock().unwrap();

        if cache.pop(key).is_some() {
            stats.entry_count = stats.entry_count.saturating_sub(1);
            debug!(
                category = "cache",
                cache_key = %key,
                "Cache entry deleted"
            );
        }
    }

    /// Clear entire cache
    ///
    /// Removes all cached entries. Use sparingly in production.
    #[allow(dead_code)]
    pub fn clear(&self) {
        let mut cache = self.cache.lock().unwrap();
        let mut stats = self.stats.lock().unwrap();

        cache.clear();
        *stats = CacheStats::default();
        info!(category = "cache", "Cache cleared");
    }

    /// Get cache statistics
    ///
    /// Returns current cache metrics for monitoring.
    pub fn stats(&self) -> CacheStats {
        self.stats.lock().unwrap().clone()
    }

    /// Get current cache size
    pub fn len(&self) -> usize {
        self.cache.lock().unwrap().len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.lock().unwrap().is_empty()
    }
}

// ==================================================================================
// CACHE KEY GENERATION
// ==================================================================================

/// Generate cache key from request parameters
///
/// Format: `gemini:{model}:{thinking_level}:{temp}:{top_p}:{max_tokens}:{prompt_hash}`
///
/// # Arguments
///
/// * `model` - Model name (e.g., "gemini-3-flash")
/// * `messages` - Request messages (used for prompt hash)
/// * `thinking_level` - Thinking level ("MINIMAL"|"LOW"|"MEDIUM"|"HIGH"|"NONE")
/// * `temperature` - Temperature parameter
/// * `top_p` - Top-p parameter
/// * `max_tokens` - Max tokens parameter
///
/// # Returns
///
/// Cache key string suitable for storage and lookup
///
/// # Example
///
/// ```rust
/// let messages = json!([{"role": "user", "content": "Hello"}]);
/// let key = generate_cache_key(
///     "gemini-3-flash",
///     &messages,
///     "MEDIUM",
///     0.7,
///     0.9,
///     1000,
/// );
/// // Returns: "gemini:gemini-3-flash:MEDIUM:0.7:0.9:1000:abc123..."
/// ```
pub fn generate_cache_key(
    model: &str,
    messages: &Value,
    thinking_level: &str,
    temperature: f32,
    top_p: f32,
    max_tokens: i32,
) -> String {
    // Hash the messages to create a deterministic prompt signature
    let mut hasher = Sha256::new();
    let messages_str = serde_json::to_string(messages).unwrap_or_default();
    hasher.update(messages_str.as_bytes());
    let hash = hasher.finalize();
    let prompt_hash = format!("{:x}", hash);

    // Take first 16 characters of hash for readability
    let short_hash = &prompt_hash[..16];

    format!(
        "gemini:{}:{}:{:.2}:{:.2}:{}:{}",
        model, thinking_level, temperature, top_p, max_tokens, short_hash
    )
}

// ==================================================================================
// TESTS
// ==================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ==================================================================================
    // TEST SUITE: Cache Key Generation
    // ==================================================================================

    #[test]
    fn test_cache_key_format() {
        let messages = json!([{"role": "user", "content": "Hello"}]);
        let key = generate_cache_key("gemini-3-flash", &messages, "MEDIUM", 0.7, 0.9, 1000);

        assert!(key.starts_with("gemini:gemini-3-flash:MEDIUM:"));
        assert!(key.contains("0.70:0.90:1000:"));
        assert_eq!(
            key.split(':').count(),
            7,
            "Key should have 7 colon-separated parts"
        );
    }

    #[test]
    fn test_cache_key_deterministic() {
        let messages = json!([{"role": "user", "content": "Test"}]);
        let key1 = generate_cache_key("gemini-3-flash", &messages, "MEDIUM", 0.7, 0.9, 1000);
        let key2 = generate_cache_key("gemini-3-flash", &messages, "MEDIUM", 0.7, 0.9, 1000);
        assert_eq!(
            key1, key2,
            "Same parameters should produce same cache key"
        );
    }

    #[test]
    fn test_cache_key_different_prompts() {
        let messages1 = json!([{"role": "user", "content": "Hello"}]);
        let messages2 = json!([{"role": "user", "content": "World"}]);
        let key1 = generate_cache_key("gemini-3-flash", &messages1, "MEDIUM", 0.7, 0.9, 1000);
        let key2 = generate_cache_key("gemini-3-flash", &messages2, "MEDIUM", 0.7, 0.9, 1000);
        assert_ne!(
            key1, key2,
            "Different prompts should produce different keys"
        );
    }

    #[test]
    fn test_cache_key_different_levels() {
        let messages = json!([{"role": "user", "content": "Test"}]);
        let key1 = generate_cache_key("gemini-3-flash", &messages, "LOW", 0.7, 0.9, 1000);
        let key2 = generate_cache_key("gemini-3-flash", &messages, "HIGH", 0.7, 0.9, 1000);
        assert_ne!(
            key1, key2,
            "Different thinking levels should produce different keys"
        );
    }

    #[test]
    fn test_cache_key_different_models() {
        let messages = json!([{"role": "user", "content": "Test"}]);
        let key1 = generate_cache_key("gemini-3-flash", &messages, "MEDIUM", 0.7, 0.9, 1000);
        let key2 = generate_cache_key("gemini-3-pro", &messages, "MEDIUM", 0.7, 0.9, 1000);
        assert_ne!(
            key1, key2,
            "Different models should produce different keys"
        );
    }

    #[test]
    fn test_cache_key_different_params() {
        let messages = json!([{"role": "user", "content": "Test"}]);
        let key1 = generate_cache_key("gemini-3-flash", &messages, "MEDIUM", 0.7, 0.9, 1000);
        let key2 = generate_cache_key("gemini-3-flash", &messages, "MEDIUM", 0.8, 0.9, 1000);
        assert_ne!(
            key1, key2,
            "Different temperature should produce different keys"
        );
    }

    // ==================================================================================
    // TEST SUITE: Response Cache
    // ==================================================================================

    #[test]
    fn test_cache_hit() {
        let cache = ResponseCache::new(10, 3600);
        let key = "test_key".to_string();
        let value = json!({"result": "test", "status": "ok"});

        // Put value in cache
        cache.put(key.clone(), value.clone());

        // Get should return the value
        let retrieved = cache.get(&key);
        assert!(retrieved.is_some(), "Cache get should return data");
        assert_eq!(retrieved.unwrap(), value);

        // Stats should reflect hit
        let stats = cache.stats();
        assert_eq!(stats.hits, 1, "Should have 1 hit");
        assert_eq!(stats.misses, 0, "Should have 0 misses");
        assert_eq!(stats.entry_count, 1, "Should have 1 entry");
    }

    #[test]
    fn test_cache_miss() {
        let cache = ResponseCache::new(10, 3600);

        // Get non-existent key
        let result = cache.get("nonexistent");
        assert!(result.is_none(), "Should return None for cache miss");

        // Stats should reflect miss
        let stats = cache.stats();
        assert_eq!(stats.hits, 0, "Should have 0 hits");
        assert_eq!(stats.misses, 1, "Should have 1 miss");
    }

    #[test]
    fn test_cache_ttl_expiration() {
        let cache = ResponseCache::new(10, 1); // 1 second TTL
        let key = "test_key".to_string();
        let value = json!({"result": "test"});

        // Put value in cache
        cache.put(key.clone(), value.clone());

        // Immediate get should succeed
        let result = cache.get(&key);
        assert!(result.is_some(), "Should get cached entry immediately");

        // Wait for expiration
        std::thread::sleep(Duration::from_secs(2));

        // Get after expiration should return None
        let result = cache.get(&key);
        assert!(result.is_none(), "Expired entry should return None");

        // Stats should reflect cleanup
        let stats = cache.stats();
        assert_eq!(stats.entry_count, 0, "Expired entry should be removed");
        assert_eq!(stats.hits, 1, "First get was a hit");
        assert_eq!(stats.misses, 1, "Second get was a miss (expired)");
    }

    #[test]
    fn test_cache_key_uniqueness() {
        let messages = json!([{"role": "user", "content": "Test"}]);

        // Test matrix from Story-013-05 AC-2
        let key1 = generate_cache_key("gemini-3-flash", &messages, "MEDIUM", 0.7, 0.9, 1000);
        let key2 = generate_cache_key("gemini-3-flash", &messages, "HIGH", 0.7, 0.9, 1000);
        let key3 = generate_cache_key("gemini-3-pro", &messages, "MEDIUM", 0.7, 0.9, 1000);
        let key4 = generate_cache_key("gemini-3-flash", &messages, "MEDIUM", 0.7, 0.9, 1000);

        // Different levels should produce different keys
        assert_ne!(key1, key2, "MEDIUM vs HIGH should differ");
        // Different models should produce different keys
        assert_ne!(key1, key3, "Flash vs Pro should differ");
        // Identical params should produce same key
        assert_eq!(key1, key4, "Identical params should match");
    }

    #[test]
    fn test_lru_eviction() {
        let cache = ResponseCache::new(3, 3600); // Capacity of 3

        // Add 3 entries
        for i in 0..3 {
            cache.put(format!("key_{}", i), json!({"value": i}));
        }

        assert_eq!(cache.len(), 3, "Cache should have 3 entries");

        // Add 4th entry (should evict oldest)
        cache.put("key_3".to_string(), json!({"value": 3}));

        assert_eq!(cache.len(), 3, "Cache should still have 3 entries");

        // key_0 should be evicted (LRU)
        let result = cache.get("key_0");
        assert!(result.is_none(), "Oldest entry should be evicted");

        // key_3 should exist
        let result = cache.get("key_3");
        assert!(result.is_some(), "Newest entry should exist");

        // Stats should reflect eviction
        let stats = cache.stats();
        assert_eq!(stats.evictions, 1, "Should have 1 eviction");
    }

    #[test]
    fn test_cache_delete() {
        let cache = ResponseCache::new(10, 3600);
        let key = "test_key".to_string();
        let value = json!({"result": "test"});

        // Put value in cache
        cache.put(key.clone(), value);

        // Delete
        cache.delete(&key);

        // Get should return None
        let result = cache.get(&key);
        assert!(result.is_none(), "Deleted entry should not be found");

        // Stats should reflect deletion
        let stats = cache.stats();
        assert_eq!(stats.entry_count, 0, "Entry count should be 0");
    }

    #[test]
    fn test_cache_clear() {
        let cache = ResponseCache::new(10, 3600);

        // Add multiple entries
        for i in 0..5 {
            cache.put(format!("key_{}", i), json!({"value": i}));
        }

        assert_eq!(cache.len(), 5, "Cache should have 5 entries");

        // Clear all
        cache.clear();

        assert_eq!(cache.len(), 0, "Cache should be empty");

        // All entries should be gone
        for i in 0..5 {
            let result = cache.get(&format!("key_{}", i));
            assert!(result.is_none(), "Entry {} should be cleared", i);
        }

        // Stats should be reset
        let stats = cache.stats();
        assert_eq!(stats.entry_count, 0);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 5); // From the 5 get calls above
    }

    #[test]
    fn test_cache_stats_hit_rate() {
        let cache = ResponseCache::new(10, 3600);
        let value = json!({"result": "test"});

        cache.put("test_key".to_string(), value);

        // 3 hits
        for _ in 0..3 {
            let _ = cache.get("test_key");
        }

        // 2 misses
        for _ in 0..2 {
            let _ = cache.get("nonexistent");
        }

        let stats = cache.stats();
        assert_eq!(stats.hits, 3);
        assert_eq!(stats.misses, 2);
        assert_eq!(stats.hit_rate(), 0.6); // 3/5 = 60%
        assert_eq!(stats.miss_rate(), 0.4); // 2/5 = 40%
    }
}
