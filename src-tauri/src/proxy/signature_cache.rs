use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

// Node.js proxy uses 2 hours TTL
const SIGNATURE_TTL: Duration = Duration::from_secs(2 * 60 * 60);
const MIN_SIGNATURE_LENGTH: usize = 50;

/// Cache entry with timestamp for TTL
#[derive(Clone, Debug)]
struct CacheEntry<T> {
    data: T,
    timestamp: SystemTime,
}

impl<T> CacheEntry<T> {
    fn new(data: T) -> Self {
        Self {
            data,
            timestamp: SystemTime::now(),
        }
    }

    fn is_expired(&self) -> bool {
        self.timestamp.elapsed().unwrap_or(Duration::ZERO) > SIGNATURE_TTL
    }
}

/// Double-layer signature cache to handle:
/// 1. Signature recovery for tool calls (when clients strip them)
/// 2. Cross-model compatibility checks (preventing Claude signatures on Gemini models)
pub struct SignatureCache {
    /// Layer 1: Tool Use ID -> Thinking Signature
    /// Key: tool_use_id (e.g., "toolu_01...")
    /// Value: The thought signature that generated this tool call
    tool_signatures: Mutex<HashMap<String, CacheEntry<String>>>,

    /// Layer 2: Signature -> Model Family
    /// Key: thought signature string
    /// Value: Model family identifier (e.g., "claude-3-5-sonnet", "gemini-2.0-flash")
    thinking_families: Mutex<HashMap<String, CacheEntry<String>>>,
}

impl SignatureCache {
    fn new() -> Self {
        Self {
            tool_signatures: Mutex::new(HashMap::new()),
            thinking_families: Mutex::new(HashMap::new()),
        }
    }

    /// Global singleton instance
    pub fn global() -> &'static SignatureCache {
        static INSTANCE: OnceLock<SignatureCache> = OnceLock::new();
        INSTANCE.get_or_init(SignatureCache::new)
    }

    /// Store a tool call signature
    pub fn cache_tool_signature(&self, tool_use_id: &str, signature: String) {
        if signature.len() < MIN_SIGNATURE_LENGTH {
            return;
        }
        
        if let Ok(mut cache) = self.tool_signatures.lock() {
            tracing::debug!("[SignatureCache] Caching tool signature for id: {}", tool_use_id);
            cache.insert(tool_use_id.to_string(), CacheEntry::new(signature));
            
            // Clean up expired entries occasionally (simple approach: unexpected check)
            // In a production system we might want a dedicated background task
            if cache.len() > 1000 {
                cache.retain(|_, v| !v.is_expired());
            }
        }
    }

    /// Retrieve a signature for a tool_use_id
    pub fn get_tool_signature(&self, tool_use_id: &str) -> Option<String> {
        if let Ok(cache) = self.tool_signatures.lock() {
            if let Some(entry) = cache.get(tool_use_id) {
                if !entry.is_expired() {
                    tracing::debug!("[SignatureCache] Hit tool signature for id: {}", tool_use_id);
                    return Some(entry.data.clone());
                }
            }
        }
        None
    }

    /// Store model family for a signature
    pub fn cache_thinking_family(&self, signature: String, family: String) {
        if signature.len() < MIN_SIGNATURE_LENGTH {
            return;
        }

        if let Ok(mut cache) = self.thinking_families.lock() {
            tracing::debug!("[SignatureCache] Caching thinking family for sig (len={}): {}", signature.len(), family);
            cache.insert(signature, CacheEntry::new(family));
            
            if cache.len() > 1000 {
                cache.retain(|_, v| !v.is_expired());
            }
        }
    }

    /// Get model family for a signature
    pub fn get_signature_family(&self, signature: &str) -> Option<String> {
        if let Ok(cache) = self.thinking_families.lock() {
            if let Some(entry) = cache.get(signature) {
                if !entry.is_expired() {
                    return Some(entry.data.clone());
                } else {
                    tracing::debug!("[SignatureCache] Signature family entry expired");
                }
            }
        }
        None
    }

    /// Clear all caches (for testing or manual reset)
    pub fn clear(&self) {
        if let Ok(mut cache) = self.tool_signatures.lock() {
            cache.clear();
        }
        if let Ok(mut cache) = self.thinking_families.lock() {
            cache.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_tool_signature_cache() {
        let cache = SignatureCache::new();
        let sig = "x".repeat(60); // Valid length
        
        cache.cache_tool_signature("tool_1", sig.clone());
        assert_eq!(cache.get_tool_signature("tool_1"), Some(sig));
        assert_eq!(cache.get_tool_signature("tool_2"), None);
    }

    #[test]
    fn test_min_length() {
        let cache = SignatureCache::new();
        cache.cache_tool_signature("tool_short", "short".to_string());
        assert_eq!(cache.get_tool_signature("tool_short"), None);
    }

    #[test]
    fn test_thinking_family() {
        let cache = SignatureCache::new();
        let sig = "y".repeat(60);
        
        cache.cache_thinking_family(sig.clone(), "claude".to_string());
        assert_eq!(cache.get_signature_family(&sig), Some("claude".to_string()));
    }
}
