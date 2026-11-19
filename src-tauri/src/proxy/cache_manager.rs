//! Context Cache Manager — Tools Cache & Prefix Hash
//!
//! 维护 Tools 结构处理结果缓存以及组合前缀哈希计算。
//! 用于跨 session 复用相同 tool schema 的处理结果，减少冗余序列化与转换。
//!
//! 功能:
//!   - Tools Cache: raw_tools_json → processed_tools (跨 session 复用)
//!   - compute_prefix_hash: 计算 (systemInstruction + tools) 组合哈希供跟踪与链路日志使用

use dashmap::DashMap;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

// ===== Cache Limits =====
const TOOLS_CACHE_LIMIT: usize = 100;

// ===== TTL Constants =====
/// Tools Cache TTL: 30 min — 比最终内容缓存更长，因为不随 session 变化
const TOOLS_CACHE_TTL: Duration = Duration::from_secs(30 * 60);

// ===== Tools Cache =====

/// Tools Cache 条目: raw tools JSON → processed tools
#[derive(Debug, Clone)]
struct ToolsCacheEntry {
    /// 处理后的 tools JSON (序列化为字符串，使用时反序列化)
    tools_json: String,
    /// 创建/更新时间
    timestamp: Instant,
    /// 命中次数
    hit_count: u64,
}

// ===== Stats =====

/// 统计信息
#[derive(Debug, Clone, Default)]
pub struct LayerStats {
    /// Tools 缓存统计
    pub tools_total: u64,
    pub tools_hits: u64,
    pub tools_misses: u64,
    /// 当前活跃条目数
    pub active_tools_entries: usize,
}

// ===== CacheManager =====

/// Context Cache Manager
pub struct CacheManager {
    /// raw tools hash → processed tools JSON
    tools_cache: DashMap<String, ToolsCacheEntry>,
    /// Tools 统计
    tools_stats: std::sync::RwLock<(u64, u64, u64)>,
    /// 会话最近一次巨型冷请求完成记录: session_id -> (完成时间戳 Instant, 输入 tokens)
    heavy_miss_completions: DashMap<String, (Instant, u32)>,
}

impl Default for CacheManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CacheManager {
    /// 创建新的 CacheManager
    pub fn new() -> Self {
        Self {
            tools_cache: DashMap::new(),
            tools_stats: std::sync::RwLock::new((0, 0, 0)),
            heavy_miss_completions: DashMap::new(),
        }
    }

    // ===== Shared Utilities =====

    /// SHA256 快速哈希
    fn sha256_hex(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    /// 检查 Instant 是否已过期
    fn is_expired(timestamp: Instant, ttl: Duration) -> bool {
        timestamp.elapsed() > ttl
    }

    // ===== Tools Cache =====

    /// 查找已缓存的 processed tools
    ///
    /// 返回 Some(tools_json_string) 如果缓存有效，否则 None
    pub fn lookup_tools(&self, raw_hash: &str) -> Option<String> {
        if let Ok(mut stats) = self.tools_stats.write() {
            stats.0 += 1;
        }

        let mut hit = false;
        let mut expired = false;
        let mut json = None;

        if let Some(entry) = self.tools_cache.get(raw_hash) {
            if !Self::is_expired(entry.timestamp, TOOLS_CACHE_TTL) {
                hit = true;
                json = Some(entry.tools_json.clone());
            } else {
                expired = true;
            }
        }

        if hit {
            if let Ok(mut stats) = self.tools_stats.write() {
                stats.1 += 1;
            }
            if let Some(mut e) = self.tools_cache.get_mut(raw_hash) {
                e.hit_count += 1;
            }
            tracing::debug!(
                "[CacheManager:Tools] HIT hash={}",
                &raw_hash[..raw_hash.len().min(16)]
            );
            return json;
        }

        if expired {
            self.tools_cache.remove_if(raw_hash, |_, entry| {
                Self::is_expired(entry.timestamp, TOOLS_CACHE_TTL)
            });
            if let Ok(mut stats) = self.tools_stats.write() {
                stats.2 += 1;
            }
            tracing::debug!(
                "[CacheManager:Tools] EXPIRED hash={}",
                &raw_hash[..raw_hash.len().min(16)]
            );
            return None;
        }

        if let Ok(mut stats) = self.tools_stats.write() {
            stats.2 += 1;
        }
        tracing::debug!(
            "[CacheManager:Tools] MISS hash={}",
            &raw_hash[..raw_hash.len().min(16)]
        );
        None
    }

    /// 存入 Tools: raw → processed tools JSON
    pub fn cache_tools(&self, raw_hash: String, tools_json: String) {
        let entry = ToolsCacheEntry {
            tools_json,
            timestamp: Instant::now(),
            hit_count: 0,
        };

        self.tools_cache.insert(raw_hash.clone(), entry);
        tracing::debug!(
            "[CacheManager:Tools] INSERT hash={}",
            &raw_hash[..raw_hash.len().min(16)]
        );

        if self.tools_cache.len() > TOOLS_CACHE_LIMIT {
            let before = self.tools_cache.len();
            self.tools_cache
                .retain(|_, v| !Self::is_expired(v.timestamp, TOOLS_CACHE_TTL));
            let after = self.tools_cache.len();
            if before != after {
                tracing::debug!(
                    "[CacheManager:Tools] LRU cleanup: {} → {} (limit: {})",
                    before,
                    after,
                    TOOLS_CACHE_LIMIT
                );
            }
        }
    }

    /// 计算 Tools key: SHA256(raw tools JSON string)
    pub fn compute_tools_key(raw_tools_json: &str) -> String {
        Self::sha256_hex(raw_tools_json.as_bytes())
    }

    /// 计算稳定的组合前缀哈希
    ///
    /// 输入: systemInstruction 和 tools 的 JSON 字符串
    pub fn compute_prefix_hash(si_json: &str, tools_json: &str) -> String {
        let si_hash = Self::sha256_hex(si_json.as_bytes());
        let tools_hash = if tools_json.is_empty() {
            String::from("no-tools")
        } else {
            Self::sha256_hex(tools_json.as_bytes())
        };
        // 组合: SHA256(si_hash + "::" + tools_hash)
        Self::sha256_hex(format!("{}::{}", si_hash, tools_hash).as_bytes())
    }

    // ===== Stats & Management =====

    /// 获取统计信息
    pub fn get_layer_stats(&self) -> LayerStats {
        let tools = self.tools_stats.read().unwrap();

        LayerStats {
            tools_total: tools.0,
            tools_hits: tools.1,
            tools_misses: tools.2,
            active_tools_entries: self.tools_cache.len(),
        }
    }

    /// 淘汰所有过期条目
    pub fn evict_expired(&self) -> usize {
        let tools_before = self.tools_cache.len();
        self.tools_cache
            .retain(|_, v| !Self::is_expired(v.timestamp, TOOLS_CACHE_TTL));

        // 巨型冷请求完成记录 60 秒后自动失效
        self.heavy_miss_completions
            .retain(|_, (ts, _)| ts.elapsed() < Duration::from_secs(60));

        let total = tools_before - self.tools_cache.len();
        if total > 0 {
            tracing::debug!("[CacheManager] Evicted {} expired tools entries", total);
        }
        total
    }

    /// 记录某会话刚完成了一次未命中缓存的巨型长上下文请求（构建了新 KV Cache）
    pub fn record_heavy_miss_completion(&self, session_id: &str, input_tokens: u32) {
        if input_tokens >= 16_000 && !session_id.is_empty() {
            // LRU 简易清理：超过 200 个会话记录时清理过期
            if self.heavy_miss_completions.len() > 200 {
                self.heavy_miss_completions
                    .retain(|_, (ts, _)| ts.elapsed() < Duration::from_secs(60));
            }
            self.heavy_miss_completions
                .insert(session_id.to_string(), (Instant::now(), input_tokens));
            tracing::debug!(
                "[Cache-Opt:Settling] Recorded heavy uncached completion for session={}, tokens={}",
                session_id,
                input_tokens
            );
        }
    }

    /// 检查某会话是否需要 KV 缓存落盘平滑延迟，并返回需补齐等待的毫秒数。
    /// 触发条件：
    /// 1. 当前请求前缀估算 tokens >= 16_000；
    /// 2. 上一轮同 Session 刚发生巨型冷请求完成；
    /// 3. 距离完成时间间隔 < settling_delay_ms；
    pub fn check_and_consume_settling_delay(
        &self,
        session_id: &str,
        current_tokens: u32,
        settling_delay_ms: u64,
    ) -> Option<u64> {
        if settling_delay_ms == 0 || session_id.is_empty() || current_tokens < 16_000 {
            return None;
        }

        if let Some((_, (finish_time, prior_tokens))) =
            self.heavy_miss_completions.remove(session_id)
        {
            let elapsed_ms = finish_time.elapsed().as_millis() as u64;
            if elapsed_ms < settling_delay_ms {
                let delay = settling_delay_ms.saturating_sub(elapsed_ms);
                tracing::info!(
                    "[Cache-Opt:Settling] Session {} completed heavy uncached request (prior {} toks, current {} toks) {}ms ago. Applying settling delay of {}ms to avoid cache write race.",
                    session_id,
                    prior_tokens,
                    current_tokens,
                    elapsed_ms,
                    delay
                );
                return Some(delay);
            }
        }
        None
    }

    /// 清空缓存和统计
    pub fn clear(&self) {
        self.tools_cache.clear();
        self.heavy_miss_completions.clear();
        if let Ok(mut s) = self.tools_stats.write() {
            *s = (0, 0, 0);
        }
        tracing::info!("[CacheManager] Tools cache cleared");
    }
}

// ===== Global Singleton =====

use std::sync::LazyLock;
static GLOBAL_CACHE_MANAGER: LazyLock<CacheManager> = LazyLock::new(CacheManager::new);

/// 获取全局 CacheManager 单例
pub fn global_cache_manager() -> &'static CacheManager {
    &GLOBAL_CACHE_MANAGER
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tools_cache_insert_and_lookup() {
        let cm = CacheManager::new();
        let raw = r#"[{"functionDeclarations":[{"name":"tool_a"}]}]"#;
        let key = CacheManager::compute_tools_key(raw);

        assert!(cm.lookup_tools(&key).is_none());

        cm.cache_tools(key.clone(), raw.to_string());
        assert_eq!(cm.lookup_tools(&key), Some(raw.to_string()));
    }

    #[test]
    fn test_tools_cache_hit_count() {
        let cm = CacheManager::new();
        let key = CacheManager::compute_tools_key("[{}]");
        cm.cache_tools(key.clone(), "[{}]".to_string());

        cm.lookup_tools(&key);
        cm.lookup_tools(&key);
        cm.lookup_tools(&key);

        let stats = cm.get_layer_stats();
        assert_eq!(stats.tools_total, 3);
        assert_eq!(stats.tools_hits, 3);
    }

    #[test]
    fn test_compute_prefix_hash_deterministic() {
        let si1 = r#"{"role":"user","parts":[{"text":"system prompt"}]}"#;
        let tools1 = r#"[{"functionDeclarations":[{"name":"test_tool"}]}]"#;

        let hash1 = CacheManager::compute_prefix_hash(si1, tools1);
        let hash2 = CacheManager::compute_prefix_hash(si1, tools1);
        assert_eq!(hash1, hash2, "Same inputs must produce same hash");
    }

    #[test]
    fn test_compute_prefix_hash_different_si() {
        let si_a = r#"{"role":"user","parts":[{"text":"system A"}]}"#;
        let si_b = r#"{"role":"user","parts":[{"text":"system B"}]}"#;
        let tools = r#"[{"functionDeclarations":[]}]"#;

        let hash_a = CacheManager::compute_prefix_hash(si_a, tools);
        let hash_b = CacheManager::compute_prefix_hash(si_b, tools);
        assert_ne!(hash_a, hash_b, "Different SI must produce different hash");
    }

    #[test]
    fn test_compute_prefix_hash_different_tools() {
        let si = r#"{"role":"user","parts":[{"text":"system"}]}"#;
        let tools_a = r#"[{"functionDeclarations":[{"name":"a"}]}]"#;
        let tools_b = r#"[{"functionDeclarations":[{"name":"b"}]}]"#;

        let hash_a = CacheManager::compute_prefix_hash(si, tools_a);
        let hash_b = CacheManager::compute_prefix_hash(si, tools_b);
        assert_ne!(
            hash_a, hash_b,
            "Different tools must produce different hash"
        );
    }

    #[test]
    fn test_compute_prefix_hash_empty_tools() {
        let si = "some si";
        let tools_empty = "";

        let hash = CacheManager::compute_prefix_hash(si, tools_empty);
        assert!(!hash.is_empty());
    }

    #[test]
    fn test_clear() {
        let cm = CacheManager::new();

        cm.cache_tools("tools".to_string(), "{}".to_string());
        assert_eq!(cm.get_layer_stats().active_tools_entries, 1);

        cm.clear();

        let stats = cm.get_layer_stats();
        assert_eq!(stats.active_tools_entries, 0);
        assert_eq!(stats.tools_total, 0);
    }

    #[test]
    fn test_global_singleton() {
        let cm = global_cache_manager();
        cm.clear();
        let stats = cm.get_layer_stats();
        assert_eq!(stats.tools_total, 0);
        assert_eq!(stats.active_tools_entries, 0);
    }

    #[test]
    fn test_heavy_miss_completion_and_settling_delay() {
        let cm = CacheManager::new();
        let sid = "sess-settling-test";

        // 1. 小请求 (<16k) 不触发记录
        cm.record_heavy_miss_completion(sid, 8000);
        assert!(cm
            .check_and_consume_settling_delay(sid, 20000, 1000)
            .is_none());

        // 2. 巨型未命中请求 (>=16k) 触发记录
        cm.record_heavy_miss_completion(sid, 28000);

        // 3. 当前请求若是小请求 (<16k)，不触发延迟
        // 注意：remove 只有在满足当前请求也是巨型请求时才消费
        assert!(cm
            .check_and_consume_settling_delay(sid, 5000, 1000)
            .is_none());

        // 4. 当前请求是连续巨型请求，成功触发平滑延迟（且单次消费）
        let delay = cm.check_and_consume_settling_delay(sid, 32000, 1000);
        assert!(delay.is_some());
        assert!(delay.unwrap() <= 1000);

        // 5. 已消费过，下一次不再重复触发
        assert!(cm
            .check_and_consume_settling_delay(sid, 36000, 1000)
            .is_none());

        // 6. delay_ms 为 0 时直接关闭
        cm.record_heavy_miss_completion(sid, 28000);
        assert!(cm.check_and_consume_settling_delay(sid, 32000, 0).is_none());
    }
}
