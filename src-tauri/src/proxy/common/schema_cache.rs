use serde_json::Value;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Instant;
use once_cell::sync::Lazy;

/// 缓存条目
#[derive(Clone)]
struct CacheEntry {
    /// 清洗后的 Schema
    schema: Value,
    /// 最后使用时间
    last_used: Instant,
    /// 命中次数
    hit_count: usize,
}

/// Schema 缓存
struct SchemaCache {
    /// 缓存存储 (key: cache_key, value: CacheEntry)
    cache: HashMap<String, CacheEntry>,
    /// 缓存统计
    stats: CacheStats,
}

/// 缓存统计
#[derive(Default, Clone, Debug)]
pub struct CacheStats {
    /// 总请求次数
    pub total_requests: usize,
    /// 缓存命中次数
    pub cache_hits: usize,
    /// 缓存未命中次数
    pub cache_misses: usize,
}

impl CacheStats {
    /// 计算缓存命中率
    pub fn hit_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.cache_hits as f64 / self.total_requests as f64
        }
    }
}

impl SchemaCache {
    fn new() -> Self {
        Self {
            cache: HashMap::new(),
            stats: CacheStats::default(),
        }
    }

    /// 获取缓存条目
    fn get(&mut self, key: &str) -> Option<Value> {
        self.stats.total_requests += 1;
        
        if let Some(entry) = self.cache.get_mut(key) {
            // 更新使用时间和命中次数
            entry.last_used = Instant::now();
            entry.hit_count += 1;
            self.stats.cache_hits += 1;
            Some(entry.schema.clone())
        } else {
            self.stats.cache_misses += 1;
            None
        }
    }

    /// 插入缓存条目
    fn insert(&mut self, key: String, schema: Value) {
        // 检查缓存大小,如果超过限制则清理
        const MAX_CACHE_SIZE: usize = 1000;
        if self.cache.len() >= MAX_CACHE_SIZE {
            self.evict_lru();
        }

        let entry = CacheEntry {
            schema,
            last_used: Instant::now(),
            hit_count: 0,
        };
        self.cache.insert(key, entry);
    }

    /// LRU 淘汰策略: 移除最久未使用的条目
    fn evict_lru(&mut self) {
        if self.cache.is_empty() {
            return;
        }

        // 找到最久未使用的条目
        let oldest_key = self
            .cache
            .iter()
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(key, _)| key.clone());

        if let Some(key) = oldest_key {
            self.cache.remove(&key);
        }
    }

    /// 获取缓存统计
    fn stats(&self) -> CacheStats {
        self.stats.clone()
    }

    /// 清空缓存
    fn clear(&mut self) {
        self.cache.clear();
        self.stats = CacheStats::default();
    }
}

/// 全局 Schema 缓存实例
static SCHEMA_CACHE: Lazy<RwLock<SchemaCache>> = Lazy::new(|| {
    RwLock::new(SchemaCache::new())
});

/// 计算 Schema 的哈希值
/// 
/// 使用 SHA-256 算法计算 Schema 的哈希值,确保相同的 Schema 产生相同的哈希
fn compute_schema_hash(schema: &Value) -> String {
    use sha2::{Sha256, Digest};
    
    let mut hasher = Sha256::new();
    // 使用紧凑格式序列化以提高一致性
    let schema_str = schema.to_string();
    hasher.update(schema_str.as_bytes());
    
    // 返回十六进制字符串的前 16 位 (足够唯一)
    format!("{:x}", hasher.finalize())[..16].to_string()
}

/// 带缓存的 Schema 清洗
/// 
/// 这是推荐的清洗入口,支持缓存优化
/// 
/// # Arguments
/// * `schema` - 待清洗的 JSON Schema
/// * `tool_name` - 工具名称,用于缓存键
/// 
/// # Returns
/// 清洗后的 Schema
pub fn clean_json_schema_cached(schema: &mut Value, tool_name: &str) {
    // 1. 计算原始 Schema 的缓存键
    let hash = compute_schema_hash(schema);
    let cache_key = format!("{}:{}", tool_name, hash);
    
    // 2. 尝试从缓存读取
    {
        if let Ok(mut cache) = SCHEMA_CACHE.write() {
            if let Some(cached) = cache.get(&cache_key) {
                *schema = cached;
                return;
            }
        }
    }
    
    // 3. 缓存未命中,执行清洗
    super::json_schema::clean_json_schema_for_tool(schema, tool_name);
    
    // 4. 写入缓存 (使用原始哈希作为键)
    if let Ok(mut cache) = SCHEMA_CACHE.write() {
        cache.insert(cache_key, schema.clone());
    }
}

/// 获取缓存统计信息
pub fn get_cache_stats() -> CacheStats {
    SCHEMA_CACHE
        .read()
        .map(|cache| cache.stats())
        .unwrap_or_default()
}

/// 清空缓存
pub fn clear_cache() {
    if let Ok(mut cache) = SCHEMA_CACHE.write() {
        cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_compute_schema_hash() {
        let schema1 = json!({"type": "string"});
        let schema2 = json!({"type": "string"});
        let schema3 = json!({"type": "number"});
        
        let hash1 = compute_schema_hash(&schema1);
        let hash2 = compute_schema_hash(&schema2);
        let hash3 = compute_schema_hash(&schema3);
        
        // 相同的 Schema 应该产生相同的哈希
        assert_eq!(hash1, hash2);
        // 不同的 Schema 应该产生不同的哈希
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_cache_hit() {
        clear_cache();
        
        let mut schema = json!({"type": "string", "minLength": 5});
        let tool_name = "test_tool";
        
        // 第一次调用 - 缓存未命中
        clean_json_schema_cached(&mut schema, tool_name);
        
        // 第二次调用相同的 Schema - 应该缓存命中
        let mut schema2 = json!({"type": "string", "minLength": 5});
        clean_json_schema_cached(&mut schema2, tool_name);
        
        let stats = get_cache_stats();
        // 验证有缓存命中
        assert!(stats.cache_hits > 0, "Expected cache hits, got: {:?}", stats);
        assert!(stats.hit_rate() > 0.0);
    }

    #[test]
    fn test_cache_eviction() {
        clear_cache();
        
        // 插入大量条目触发淘汰
        for i in 0..1100 {
            let mut schema = json!({"type": "string", "index": i});
            let tool_name = format!("tool_{}", i);
            clean_json_schema_cached(&mut schema, &tool_name);
        }
        
        // 验证缓存大小被限制
        let stats = get_cache_stats();
        assert!(stats.total_requests > 0);
    }
}
