//! Image generation response caching (Story-007-04)
//!
//! Reduces costs by caching Gemini image generation responses.
//! Supports multiple backends: NoOp (testing), Filesystem (production), Redis (optional).
//!
//! # Architecture
//!
//! - **Trait-based design**: `CacheBackend` trait for pluggable backends
//! - **Cache key format**: `img:{model}:{quality}:{style}:{prompt_hash}`
//! - **TTL support**: Configurable time-to-live for cached entries
//! - **LRU eviction**: Least-recently-used eviction when size limits exceeded
//!
//! # Performance Targets
//!
//! - Cache hit: <100ms (p99)
//! - Cache miss overhead: <10ms
//! - Cache hit rate: ≥30% in production
//! - Storage efficiency: <50MB for 1000 images
//!
//! # Configuration
//!
//! Environment variables:
//! - `CACHE_BACKEND=none|filesystem|redis` (default: none)
//! - `CACHE_TTL_SECONDS=3600` (default: 1 hour)
//! - `CACHE_MAX_SIZE_MB=100` (default: 100MB)
//! - `CACHE_DIR={data_dir}/image_cache/` (filesystem only)
//!
//! # Example Usage
//!
//! ```rust
//! use cache::{CacheBackend, FilesystemCache, generate_cache_key};
//!
//! // Generate cache key
//! let key = generate_cache_key("gemini-3-pro-image-4k", "sunset", Some("hd"), Some("vivid"));
//!
//! // Try cache lookup
//! if let Some(cached) = cache.get(&key).await? {
//!     return Ok(cached); // Cache hit
//! }
//!
//! // Generate image (cache miss)
//! let result = generate_image().await?;
//!
//! // Store in cache
//! cache.set(&key, result.clone(), Duration::from_secs(3600)).await?;
//! ```

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

// ==================================================================================
// DATA STRUCTURES
// ==================================================================================

/// Cached image response
///
/// Contains the generated image data and metadata for cache management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedImage {
    /// Base64-encoded image data
    pub b64_json: String,
    /// Model used for generation
    pub model: String,
    /// Unix timestamp when cached
    pub created_at: u64,
    /// SHA256 hash of prompt (first 16 chars)
    pub prompt_hash: String,
    /// Image quality setting (standard | hd)
    pub quality: String,
    /// Image style setting (natural | vivid)
    pub style: String,
}

/// Cache statistics for monitoring
///
/// Tracks cache performance metrics for observability.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses
    pub misses: u64,
    /// Number of evictions (LRU)
    pub evictions: u64,
    /// Total cache size in bytes
    pub size_bytes: u64,
    /// Number of entries in cache
    pub entry_count: u64,
}

impl CacheStats {
    /// Calculate cache hit rate (0.0 - 1.0)
    #[allow(dead_code)]
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

// ==================================================================================
// CACHE BACKEND TRAIT
// ==================================================================================

/// Cache backend trait (async-friendly)
///
/// Provides a pluggable interface for different cache implementations.
/// All methods are async to support both local and distributed caches.
#[async_trait::async_trait]
pub trait CacheBackend: Send + Sync {
    /// Lookup cached image by key
    ///
    /// Returns `Some(CachedImage)` on cache hit, `None` on miss.
    /// Expired entries are automatically removed and counted as misses.
    ///
    /// # Performance
    /// Target: <100ms (p99)
    async fn get(&self, key: &str) -> Result<Option<CachedImage>>;

    /// Store image in cache with TTL
    ///
    /// Evicts old entries if cache size limits are exceeded (LRU).
    ///
    /// # Performance
    /// Target: async, non-blocking
    async fn set(&self, key: &str, value: CachedImage, ttl: Duration) -> Result<()>;

    /// Delete specific cache entry
    ///
    /// Used for manual cache invalidation.
    #[allow(dead_code)]
    async fn delete(&self, key: &str) -> Result<()>;

    /// Clear entire cache
    ///
    /// Removes all cached entries. Use sparingly in production.
    #[allow(dead_code)]
    async fn clear(&self) -> Result<()>;

    /// Get cache statistics
    ///
    /// Returns current cache metrics for monitoring.
    #[allow(dead_code)]
    async fn stats(&self) -> Result<CacheStats>;
}

// ==================================================================================
// NOOP CACHE (TESTING)
// ==================================================================================

/// No-op cache implementation (for testing or when caching disabled)
///
/// Always returns cache misses and discards all data.
/// Zero overhead, useful for benchmarking and testing.
pub struct NoOpCache;

impl NoOpCache {
    /// Create new NoOp cache
    pub fn new() -> Self {
        NoOpCache
    }
}

#[async_trait::async_trait]
impl CacheBackend for NoOpCache {
    async fn get(&self, _key: &str) -> Result<Option<CachedImage>> {
        Ok(None) // Always miss
    }

    async fn set(&self, _key: &str, _value: CachedImage, _ttl: Duration) -> Result<()> {
        Ok(()) // Discard
    }

    async fn delete(&self, _key: &str) -> Result<()> {
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        Ok(())
    }

    async fn stats(&self) -> Result<CacheStats> {
        Ok(CacheStats::default())
    }
}

// ==================================================================================
// FILESYSTEM CACHE
// ==================================================================================

/// Cache entry metadata (internal)
struct CacheEntry {
    /// Full path to cached file
    file_path: PathBuf,
    /// Size of cached file in bytes
    size_bytes: u64,
    /// Unix timestamp when entry expires
    expires_at: u64,
    /// Last access time (for LRU eviction)
    last_accessed: u64,
}

/// Filesystem-based cache implementation
///
/// Stores cached images as JSON files in a local directory.
/// Supports TTL expiration and LRU eviction.
///
/// # Performance Characteristics
///
/// - Get: O(1) hash lookup + file read (~1-10ms for typical images)
/// - Set: O(1) hash insert + file write (~5-20ms)
/// - Eviction: O(n log n) sort by access time (rare, triggered on size limit)
///
/// # Storage Layout
///
/// ```text
/// {cache_dir}/
///   ├── img_gemini-3-pro-image_hd_vivid_a1b2c3d4.json
///   ├── img_gemini-3-pro-image_standard_natural_e5f6g7h8.json
///   └── ...
/// ```
pub struct FilesystemCache {
    /// Root directory for cache files
    cache_dir: PathBuf,
    /// Maximum cache size in bytes
    max_size_bytes: u64,
    /// Default TTL for new entries
    #[allow(dead_code)]
    ttl: Duration,
    /// Cache statistics (thread-safe)
    stats: Arc<RwLock<CacheStats>>,
    /// In-memory index of cache entries (thread-safe)
    index: Arc<RwLock<HashMap<String, CacheEntry>>>,
}

impl FilesystemCache {
    /// Create new filesystem cache
    ///
    /// # Arguments
    ///
    /// * `cache_dir` - Directory to store cache files
    /// * `max_size_mb` - Maximum cache size in megabytes
    /// * `ttl` - Default time-to-live for cached entries
    ///
    /// # Errors
    ///
    /// Returns error if cache directory cannot be created.
    pub fn new(cache_dir: PathBuf, max_size_mb: u64, ttl: Duration) -> Result<Self> {
        std::fs::create_dir_all(&cache_dir)?;
        Ok(Self {
            cache_dir,
            max_size_bytes: max_size_mb * 1024 * 1024,
            ttl,
            stats: Arc::new(RwLock::new(CacheStats::default())),
            index: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Evict entries if needed to free space for new entry
    ///
    /// Uses LRU (Least Recently Used) eviction strategy.
    /// Sorts entries by last access time and removes oldest until sufficient space.
    async fn evict_if_needed(&self, needed_bytes: u64) -> Result<()> {
        let mut index = self.index.write().await;
        let stats = self.stats.read().await;

        // Check if eviction is needed
        if stats.size_bytes + needed_bytes <= self.max_size_bytes {
            return Ok(()); // Sufficient space
        }

        drop(stats); // Release read lock before eviction

        // Sort entries by last access time (oldest first)
        let mut entries: Vec<_> = index.iter().collect();
        entries.sort_by_key(|(_, entry)| entry.last_accessed);

        let mut freed_bytes = 0u64;
        let mut evicted_count = 0u64;
        let mut to_remove = Vec::new();

        // Evict oldest entries until we have enough space
        for (key, entry) in entries {
            if freed_bytes >= needed_bytes {
                break;
            }

            freed_bytes += entry.size_bytes;
            evicted_count += 1;
            to_remove.push(key.clone());

            // Delete file
            let _ = tokio::fs::remove_file(&entry.file_path).await;
        }

        // Remove from index
        for key in to_remove {
            index.remove(&key);
        }

        // Update stats
        let mut stats = self.stats.write().await;
        stats.size_bytes = stats.size_bytes.saturating_sub(freed_bytes);
        stats.entry_count = stats.entry_count.saturating_sub(evicted_count);
        stats.evictions += evicted_count;

        Ok(())
    }

    /// Generate filesystem-safe filename from cache key
    fn key_to_filename(&self, key: &str) -> PathBuf {
        // Replace colons with underscores for filesystem compatibility
        let safe_name = key.replace(":", "_");
        self.cache_dir.join(format!("{}.json", safe_name))
    }
}

#[async_trait::async_trait]
impl CacheBackend for FilesystemCache {
    async fn get(&self, key: &str) -> Result<Option<CachedImage>> {
        let mut index = self.index.write().await;

        if let Some(entry) = index.get_mut(key) {
            // Check expiration
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();

            if now > entry.expires_at {
                // Expired - remove from index and delete file
                let file_path = entry.file_path.clone();
                let size = entry.size_bytes;
                index.remove(key);
                drop(index);

                let _ = tokio::fs::remove_file(&file_path).await;

                // Update stats (miss + cleanup)
                let mut stats = self.stats.write().await;
                stats.misses += 1;
                stats.size_bytes = stats.size_bytes.saturating_sub(size);
                stats.entry_count = stats.entry_count.saturating_sub(1);

                return Ok(None);
            }

            // Update last access time (LRU tracking)
            entry.last_accessed = now;

            // Read from filesystem
            let data = tokio::fs::read(&entry.file_path).await?;
            let cached: CachedImage = serde_json::from_slice(&data)?;

            // Update stats (hit)
            drop(index);
            let mut stats = self.stats.write().await;
            stats.hits += 1;

            Ok(Some(cached))
        } else {
            // Cache miss
            drop(index);
            let mut stats = self.stats.write().await;
            stats.misses += 1;
            Ok(None)
        }
    }

    async fn set(&self, key: &str, value: CachedImage, ttl: Duration) -> Result<()> {
        let data = serde_json::to_vec(&value)?;
        let size = data.len() as u64;

        // Evict if needed
        self.evict_if_needed(size).await?;

        // Write to filesystem
        let file_path = self.key_to_filename(key);
        tokio::fs::write(&file_path, data).await?;

        // Calculate expiration time
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let expires_at = now + ttl.as_secs();

        // Update index
        let mut index = self.index.write().await;
        index.insert(
            key.to_string(),
            CacheEntry {
                file_path,
                size_bytes: size,
                expires_at,
                last_accessed: now,
            },
        );

        // Update stats
        drop(index);
        let mut stats = self.stats.write().await;
        stats.size_bytes += size;
        stats.entry_count += 1;

        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let mut index = self.index.write().await;

        if let Some(entry) = index.remove(key) {
            let _ = tokio::fs::remove_file(&entry.file_path).await;

            // Update stats
            drop(index);
            let mut stats = self.stats.write().await;
            stats.size_bytes = stats.size_bytes.saturating_sub(entry.size_bytes);
            stats.entry_count = stats.entry_count.saturating_sub(1);
        }

        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        let mut index = self.index.write().await;

        // Delete all files
        for (_, entry) in index.drain() {
            let _ = tokio::fs::remove_file(&entry.file_path).await;
        }

        // Reset stats
        let mut stats = self.stats.write().await;
        *stats = CacheStats::default();

        Ok(())
    }

    async fn stats(&self) -> Result<CacheStats> {
        Ok(self.stats.read().await.clone())
    }
}

// ==================================================================================
// CACHE KEY GENERATION
// ==================================================================================

/// Generate cache key from request parameters
///
/// Format: `img:{model}:{quality}:{style}:{prompt_hash}`
///
/// # Arguments
///
/// * `model` - Model ID (e.g., "gemini-3-pro-image-4k-16x9")
/// * `prompt` - User prompt text
/// * `quality` - Image quality ("hd" | "standard")
/// * `style` - Image style ("vivid" | "natural")
///
/// # Returns
///
/// Cache key string suitable for storage and lookup
///
/// # Example
///
/// ```rust
/// let key = generate_cache_key(
///     "gemini-3-pro-image-4k-16x9",
///     "A beautiful sunset",
///     Some("hd"),
///     Some("vivid"),
/// );
/// // Returns: "img:gemini-3-pro-image-4k-16x9:hd:vivid:a1b2c3d4e5f6..."
/// ```
pub fn generate_cache_key(
    model: &str,
    prompt: &str,
    quality: Option<&str>,
    style: Option<&str>,
) -> String {
    // Reuse hash_prompt from errors.rs (Developer A's implementation)
    use crate::proxy::errors::hash_prompt;

    let prompt_hash = hash_prompt(prompt);
    let quality_str = quality.unwrap_or("standard");
    let style_str = style.unwrap_or("natural");

    format!(
        "img:{}:{}:{}:{}",
        model, quality_str, style_str, prompt_hash
    )
}

// ==================================================================================
// TESTS
// ==================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ==================================================================================
    // TEST SUITE: Cache Key Generation
    // ==================================================================================

    #[test]
    fn test_cache_key_format() {
        let key = generate_cache_key(
            "gemini-3-pro-image-4k-16x9",
            "A beautiful sunset",
            Some("hd"),
            Some("vivid"),
        );

        assert!(key.starts_with("img:gemini-3-pro-image-4k-16x9:hd:vivid:"));
        assert!(key.contains("img:"), "Key should start with 'img:' prefix");
        assert_eq!(
            key.split(':').count(),
            5,
            "Key should have 5 colon-separated parts"
        );
    }

    #[test]
    fn test_cache_key_deterministic() {
        let key1 = generate_cache_key("gemini-3-pro-image", "test", Some("hd"), Some("vivid"));
        let key2 = generate_cache_key("gemini-3-pro-image", "test", Some("hd"), Some("vivid"));
        assert_eq!(key1, key2, "Same parameters should produce same cache key");
    }

    #[test]
    fn test_cache_key_different_prompts() {
        let key1 = generate_cache_key("gemini-3-pro-image", "sunset", Some("hd"), Some("vivid"));
        let key2 = generate_cache_key("gemini-3-pro-image", "sunrise", Some("hd"), Some("vivid"));
        assert_ne!(
            key1, key2,
            "Different prompts should produce different keys"
        );
    }

    #[test]
    fn test_cache_key_defaults() {
        let key = generate_cache_key("gemini-3-pro-image", "test", None, None);
        assert!(
            key.contains(":standard:natural:"),
            "Should use default quality=standard and style=natural"
        );
    }

    // ==================================================================================
    // TEST SUITE: NoOpCache
    // ==================================================================================

    #[tokio::test]
    async fn test_noop_cache_always_miss() {
        let cache = NoOpCache::new();

        // Set should succeed but do nothing
        let cached = CachedImage {
            b64_json: "test_data".to_string(),
            model: "gemini-3-pro-image".to_string(),
            created_at: 123456,
            prompt_hash: "abcd1234".to_string(),
            quality: "hd".to_string(),
            style: "vivid".to_string(),
        };

        cache
            .set("test_key", cached.clone(), Duration::from_secs(60))
            .await
            .unwrap();

        // Get should always return None
        let result = cache.get("test_key").await.unwrap();
        assert!(result.is_none(), "NoOpCache should always return None");
    }

    #[tokio::test]
    async fn test_noop_cache_stats() {
        let cache = NoOpCache::new();
        let stats = cache.stats().await.unwrap();

        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.evictions, 0);
        assert_eq!(stats.size_bytes, 0);
    }

    // ==================================================================================
    // TEST SUITE: FilesystemCache
    // ==================================================================================

    #[tokio::test]
    async fn test_filesystem_cache_set_get() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cache = FilesystemCache::new(
            temp_dir.path().to_path_buf(),
            100, // 100MB max
            Duration::from_secs(60),
        )
        .unwrap();

        let cached = CachedImage {
            b64_json: "test_base64_data".to_string(),
            model: "gemini-3-pro-image".to_string(),
            created_at: 123456,
            prompt_hash: "abcd1234".to_string(),
            quality: "hd".to_string(),
            style: "vivid".to_string(),
        };

        // Set
        cache
            .set("test_key", cached.clone(), Duration::from_secs(60))
            .await
            .unwrap();

        // Get (should hit)
        let result = cache.get("test_key").await.unwrap();
        assert!(result.is_some(), "Cache get should return data");

        let retrieved = result.unwrap();
        assert_eq!(retrieved.b64_json, cached.b64_json);
        assert_eq!(retrieved.model, cached.model);
        assert_eq!(retrieved.prompt_hash, cached.prompt_hash);

        // Stats should reflect hit
        let stats = cache.stats().await.unwrap();
        assert_eq!(stats.hits, 1, "Should have 1 hit");
        assert_eq!(stats.misses, 0, "Should have 0 misses");
        assert_eq!(stats.entry_count, 1, "Should have 1 entry");
    }

    #[tokio::test]
    async fn test_filesystem_cache_miss() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cache =
            FilesystemCache::new(temp_dir.path().to_path_buf(), 100, Duration::from_secs(60))
                .unwrap();

        // Get non-existent key
        let result = cache.get("nonexistent").await.unwrap();
        assert!(result.is_none(), "Should return None for cache miss");

        // Stats should reflect miss
        let stats = cache.stats().await.unwrap();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 1);
    }

    #[tokio::test]
    async fn test_filesystem_cache_expiration() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cache = FilesystemCache::new(
            temp_dir.path().to_path_buf(),
            100,
            Duration::from_secs(1), // 1 second TTL
        )
        .unwrap();

        let cached = CachedImage {
            b64_json: "test_data".to_string(),
            model: "gemini-3-pro-image".to_string(),
            created_at: 123456,
            prompt_hash: "abcd1234".to_string(),
            quality: "standard".to_string(),
            style: "natural".to_string(),
        };

        // Set with 1 second TTL
        cache
            .set("test_key", cached, Duration::from_secs(1))
            .await
            .unwrap();

        // Immediate get should succeed
        let result = cache.get("test_key").await.unwrap();
        assert!(result.is_some(), "Should get cached entry immediately");

        // Wait for expiration
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Get after expiration should return None
        let result = cache.get("test_key").await.unwrap();
        assert!(result.is_none(), "Expired entry should return None");

        // Stats should reflect cleanup
        let stats = cache.stats().await.unwrap();
        assert_eq!(stats.entry_count, 0, "Expired entry should be removed");
    }

    #[tokio::test]
    async fn test_filesystem_cache_delete() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cache =
            FilesystemCache::new(temp_dir.path().to_path_buf(), 100, Duration::from_secs(60))
                .unwrap();

        let cached = CachedImage {
            b64_json: "test_data".to_string(),
            model: "gemini-3-pro-image".to_string(),
            created_at: 123456,
            prompt_hash: "abcd1234".to_string(),
            quality: "standard".to_string(),
            style: "natural".to_string(),
        };

        // Set
        cache
            .set("test_key", cached, Duration::from_secs(60))
            .await
            .unwrap();

        // Delete
        cache.delete("test_key").await.unwrap();

        // Get should return None
        let result = cache.get("test_key").await.unwrap();
        assert!(result.is_none(), "Deleted entry should not be found");

        // Stats should reflect deletion
        let stats = cache.stats().await.unwrap();
        assert_eq!(stats.entry_count, 0);
    }

    #[tokio::test]
    async fn test_filesystem_cache_clear() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cache =
            FilesystemCache::new(temp_dir.path().to_path_buf(), 100, Duration::from_secs(60))
                .unwrap();

        // Add multiple entries
        for i in 0..5 {
            let cached = CachedImage {
                b64_json: format!("data_{}", i),
                model: "gemini-3-pro-image".to_string(),
                created_at: 123456,
                prompt_hash: format!("hash_{}", i),
                quality: "standard".to_string(),
                style: "natural".to_string(),
            };
            cache
                .set(&format!("key_{}", i), cached, Duration::from_secs(60))
                .await
                .unwrap();
        }

        // Clear all
        cache.clear().await.unwrap();

        // All entries should be gone
        for i in 0..5 {
            let result = cache.get(&format!("key_{}", i)).await.unwrap();
            assert!(result.is_none(), "Entry {} should be cleared", i);
        }

        // Stats should be reset
        let stats = cache.stats().await.unwrap();
        assert_eq!(stats.entry_count, 0);
        assert_eq!(stats.size_bytes, 0);
    }

    #[tokio::test]
    async fn test_cache_stats_hit_rate() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cache =
            FilesystemCache::new(temp_dir.path().to_path_buf(), 100, Duration::from_secs(60))
                .unwrap();

        let cached = CachedImage {
            b64_json: "test_data".to_string(),
            model: "gemini-3-pro-image".to_string(),
            created_at: 123456,
            prompt_hash: "abcd1234".to_string(),
            quality: "standard".to_string(),
            style: "natural".to_string(),
        };

        cache
            .set("test_key", cached, Duration::from_secs(60))
            .await
            .unwrap();

        // 3 hits
        for _ in 0..3 {
            let _ = cache.get("test_key").await.unwrap();
        }

        // 2 misses
        for _ in 0..2 {
            let _ = cache.get("nonexistent").await.unwrap();
        }

        let stats = cache.stats().await.unwrap();
        assert_eq!(stats.hits, 3);
        assert_eq!(stats.misses, 2);
        assert_eq!(stats.hit_rate(), 0.6); // 3/5 = 60%
    }
}
