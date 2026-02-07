use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

// Node.js proxy uses 2 hours TTL
const SIGNATURE_TTL: Duration = Duration::from_secs(2 * 60 * 60);
const MIN_SIGNATURE_LENGTH: usize = 50;

// Different cache limits for different layers
const TOOL_CACHE_LIMIT: usize = 500;      // Layer 1: Tool-specific signatures
const FAMILY_CACHE_LIMIT: usize = 200;    // Layer 2: Model family mappings
const SESSION_CACHE_LIMIT: usize = 1000;  // Layer 3: Session-based signatures (largest)

/// Cache entry with timestamp for TTL
#[derive(Clone, Debug)]
struct CacheEntry<T> {
    data: T,
    timestamp: SystemTime,
}

/// Specialized entry for session-based signatures to track message count
#[derive(Clone, Debug)]
struct SessionSignatureEntry {
    signature: String,
    message_count: usize,
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

/// Triple-layer signature cache to handle:
/// 1. Signature recovery for tool calls (when clients strip them)
/// 2. Cross-model compatibility checks (preventing Claude signatures on Gemini models)
/// 3. Session-based signature tracking (preventing cross-session pollution)
pub struct SignatureCache {
    /// Layer 1: Tool Use ID -> Thinking Signature
    /// Key: tool_use_id (e.g., "toolu_01...")
    /// Value: The thought signature that generated this tool call
    tool_signatures: Mutex<HashMap<String, CacheEntry<String>>>,

    /// Layer 2: Signature -> Model Family
    /// Key: thought signature string
    /// Value: Model family identifier (e.g., "claude-3-5-sonnet", "gemini-2.0-flash")
    thinking_families: Mutex<HashMap<String, CacheEntry<String>>>,

    /// Layer 3: Session ID -> Latest Thinking Signature (NEW)
    /// Key: session fingerprint (e.g., "sid-a1b2c3d4...")
    /// Value: The most recent valid thought signature for this session
    /// This prevents signature pollution between different conversations
    session_signatures: Mutex<HashMap<String, CacheEntry<SessionSignatureEntry>>>,
}

impl SignatureCache {
    fn new() -> Self {
        Self {
            tool_signatures: Mutex::new(HashMap::new()),
            thinking_families: Mutex::new(HashMap::new()),
            session_signatures: Mutex::new(HashMap::new()),
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
            
            // Clean up expired entries when limit is reached
            if cache.len() > TOOL_CACHE_LIMIT {
                let before = cache.len();
                cache.retain(|_, v| !v.is_expired());
                let after = cache.len();
                if before != after {
                    tracing::debug!("[SignatureCache] Tool cache cleanup: {} -> {} entries", before, after);
                }
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
            
            if cache.len() > FAMILY_CACHE_LIMIT {
                let before = cache.len();
                cache.retain(|_, v| !v.is_expired());
                let after = cache.len();
                if before != after {
                    tracing::debug!("[SignatureCache] Family cache cleanup: {} -> {} entries", before, after);
                }
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

    // ===== Layer 3: Session-based Signature Storage =====

    /// Store the latest thinking signature for a session.
    /// This is the preferred method for tracking signatures across tool loops.
    /// 
    /// # Arguments
    /// * `session_id` - Session fingerprint (e.g., "sid-a1b2c3d4...")
    /// * `signature` - The thought signature to store
    /// * `message_count` - The current message count of the conversation (to detect Rewind)
    pub fn cache_session_signature(&self, session_id: &str, signature: String, message_count: usize) {
        if signature.len() < MIN_SIGNATURE_LENGTH {
            return;
        }

        if let Ok(mut cache) = self.session_signatures.lock() {
            let should_store = match cache.get(session_id) {
                None => true,
                Some(existing) => {
                    if existing.is_expired() {
                        true
                    } else if message_count < existing.data.message_count {
                        // [CRITICAL] Rewind detected: user deleted messages or reverted to an earlier state.
                        // The cached signature is now from a "future" that no longer exists in history.
                        // We MUST force an update to prevent sending a future signature.
                        tracing::info!(
                            "[SignatureCache] Rewind detected for {}: {} -> {} messages. Forcing signature update.",
                            session_id,
                            existing.data.message_count,
                            message_count
                        );
                        true
                    } else if message_count == existing.data.message_count {
                        // Same message count: only update if the new signature is longer (more complete)
                        signature.len() > existing.data.signature.len()
                    } else {
                        // message_count > existing.data.message_count: normal progression
                        true
                    }
                }
            };

            if should_store {
                tracing::debug!(
                    "[SignatureCache] Session {} (msg_count={}) -> storing signature (len={})",
                    session_id,
                    message_count,
                    signature.len()
                );
                cache.insert(
                    session_id.to_string(), 
                    CacheEntry::new(SessionSignatureEntry { 
                        signature, 
                        message_count 
                    })
                );
            }

            // Cleanup when limit is reached (Session cache has largest limit)
            if cache.len() > SESSION_CACHE_LIMIT {
                let before = cache.len();
                cache.retain(|_, v| !v.is_expired());
                let after = cache.len();
                if before != after {
                    tracing::info!(
                        "[SignatureCache] Session cache cleanup: {} -> {} entries (limit: {})",
                        before,
                        after,
                        SESSION_CACHE_LIMIT
                    );
                }
            }
        }
    }

    /// Retrieve the latest thinking signature for a session.
    /// Returns None if not found or expired.
    pub fn get_session_signature(&self, session_id: &str) -> Option<String> {
        if let Ok(cache) = self.session_signatures.lock() {
            if let Some(entry) = cache.get(session_id) {
                if !entry.is_expired() {
                    tracing::debug!(
                        "[SignatureCache] Session {} -> HIT (len={})",
                        session_id,
                        entry.data.signature.len()
                    );
                    return Some(entry.data.signature.clone());
                } else {
                    tracing::debug!("[SignatureCache] Session {} -> EXPIRED", session_id);
                }
            }
        }
        None
    }

    pub fn delete_session_signature(&self, session_id: &str) {
        if let Ok(mut cache) = self.session_signatures.lock() {
            if cache.remove(session_id).is_some() {
                tracing::debug!("[SignatureCache] Deleted session signature for: {}", session_id);
            }
        }
    }

    /// Clear all caches (for testing or manual reset)
    #[allow(dead_code)] // Used in tests
    pub fn clear(&self) {
        if let Ok(mut cache) = self.tool_signatures.lock() {
            cache.clear();
        }
        if let Ok(mut cache) = self.thinking_families.lock() {
            cache.clear();
        }
        if let Ok(mut cache) = self.session_signatures.lock() {
            cache.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;


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

    #[test]
    fn test_session_signature() {
        let cache = SignatureCache::new();
        let sig1 = "a".repeat(60);
        let sig2 = "b".repeat(80); // Longer, should replace
        let sig3 = "c".repeat(40); // Too short, should be ignored
        
        // Initially empty
        assert!(cache.get_session_signature("sid-test123").is_none());
        
        // Store first signature
        cache.cache_session_signature("sid-test123", sig1.clone(), 5);
        assert_eq!(cache.get_session_signature("sid-test123"), Some(sig1.clone()));
        
        // Longer signature should replace (same msg count)
        cache.cache_session_signature("sid-test123", sig2.clone(), 5);
        assert_eq!(cache.get_session_signature("sid-test123"), Some(sig2.clone()));
        
        // Shorter valid signature should NOT replace (same msg count)
        cache.cache_session_signature("sid-test123", sig1.clone(), 5);
        assert_eq!(cache.get_session_signature("sid-test123"), Some(sig2.clone()));

        // Rewind: Shorter signature MUST replace if message count is lower
        cache.cache_session_signature("sid-test123", sig1.clone(), 3);
        assert_eq!(cache.get_session_signature("sid-test123"), Some(sig1.clone()));
        
        // Too short signature should be ignored entirely (even if rewind)
        cache.cache_session_signature("sid-test123", sig3, 1);
        assert_eq!(cache.get_session_signature("sid-test123"), Some(sig1));
        
        // Different session should be isolated
        assert!(cache.get_session_signature("sid-other").is_none());
    }

    #[test]
    fn test_clear_all_caches() {
        let cache = SignatureCache::new();
        let sig = "x".repeat(60);
        
        cache.cache_tool_signature("tool_1", sig.clone());
        cache.cache_thinking_family(sig.clone(), "model".to_string());
        cache.cache_session_signature("sid-1", sig.clone(), 1);
        
        assert!(cache.get_tool_signature("tool_1").is_some());
        assert!(cache.get_signature_family(&sig).is_some());
        assert!(cache.get_session_signature("sid-1").is_some());
        
        cache.clear();
        
        assert!(cache.get_tool_signature("tool_1").is_none());
        assert!(cache.get_signature_family(&sig).is_none());
        assert!(cache.get_session_signature("sid-1").is_none());
    }
}
