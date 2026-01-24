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

/// Session signature entry with message count for Rewind detection
#[derive(Clone, Debug)]
struct SessionSignatureEntry {
    signature: String,
    message_count: usize,
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

    /// Layer 3: Session ID -> Thinking Signature + Message Count
    /// Key: session fingerprint (e.g., "sid-a1b2c3d4...")
    /// Value: (signature, message_count) for Rewind detection
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

    /// Store thinking signature for a session with Rewind detection
    ///
    /// # Arguments
    /// * `session_id` - Session fingerprint (e.g., "sid-a1b2c3d4...")
    /// * `signature` - The thought signature to store
    /// * `message_count` - Number of messages in conversation (for Rewind detection)
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
                        // Rewind detected: message count decreased -> force replace
                        tracing::info!(
                            "[SignatureCache] Rewind detected: {} -> {} messages, invalidating",
                            existing.data.message_count,
                            message_count
                        );
                        true
                    } else if message_count == existing.data.message_count {
                        // Same count: prefer longer signature
                        signature.len() > existing.data.signature.len()
                    } else {
                        // Count increased: normal update
                        true
                    }
                }
            };

            if should_store {
                tracing::debug!(
                    "[SignatureCache] Session {} -> storing (len={}, msg={})",
                    session_id,
                    signature.len(),
                    message_count
                );
                cache.insert(
                    session_id.to_string(),
                    CacheEntry::new(SessionSignatureEntry {
                        signature,
                        message_count,
                    }),
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
                        "[SignatureCache] Session {} -> HIT (len={}, msg_count={})",
                        session_id,
                        entry.data.signature.len(),
                        entry.data.message_count
                    );
                    return Some(entry.data.signature.clone());
                } else {
                    tracing::debug!("[SignatureCache] Session {} -> EXPIRED", session_id);
                }
            }
        }
        None
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

        // Store first signature at message count 10
        cache.cache_session_signature("sid-test123", sig1.clone(), 10);
        assert_eq!(cache.get_session_signature("sid-test123"), Some(sig1.clone()));

        // Longer signature at same message count should replace
        cache.cache_session_signature("sid-test123", sig2.clone(), 10);
        assert_eq!(cache.get_session_signature("sid-test123"), Some(sig2.clone()));

        // Shorter valid signature at same message count should NOT replace
        cache.cache_session_signature("sid-test123", sig1.clone(), 10);
        assert_eq!(cache.get_session_signature("sid-test123"), Some(sig2.clone()));

        // Too short signature should be ignored entirely
        cache.cache_session_signature("sid-test123", sig3, 10);
        assert_eq!(cache.get_session_signature("sid-test123"), Some(sig2.clone()));

        // Different session should be isolated
        assert!(cache.get_session_signature("sid-other").is_none());

        // Message count increase should update
        let sig4 = "d".repeat(70);
        cache.cache_session_signature("sid-test123", sig4.clone(), 15);
        assert_eq!(cache.get_session_signature("sid-test123"), Some(sig4.clone()));

        // REWIND: Message count decrease should force replace even with shorter signature
        let sig5 = "e".repeat(60);
        cache.cache_session_signature("sid-test123", sig5.clone(), 8);
        assert_eq!(cache.get_session_signature("sid-test123"), Some(sig5));
    }

    #[test]
    fn test_rewind_detection() {
        let cache = SignatureCache::new();
        let sig_long = "x".repeat(100);
        let sig_short = "y".repeat(60);

        // Store long signature at message count 50
        cache.cache_session_signature("sid-rewind", sig_long.clone(), 50);
        assert_eq!(cache.get_session_signature("sid-rewind"), Some(sig_long.clone()));

        // Normal: shorter signature at same count should NOT replace
        cache.cache_session_signature("sid-rewind", sig_short.clone(), 50);
        assert_eq!(cache.get_session_signature("sid-rewind"), Some(sig_long.clone()));

        // REWIND: shorter signature at lower count SHOULD replace
        cache.cache_session_signature("sid-rewind", sig_short.clone(), 30);
        assert_eq!(cache.get_session_signature("sid-rewind"), Some(sig_short));
    }

    #[test]
    fn test_clear_all_caches() {
        let cache = SignatureCache::new();
        let sig = "x".repeat(60);

        cache.cache_tool_signature("tool_1", sig.clone());
        cache.cache_thinking_family(sig.clone(), "model".to_string());
        cache.cache_session_signature("sid-1", sig.clone(), 10);

        assert!(cache.get_tool_signature("tool_1").is_some());
        assert!(cache.get_signature_family(&sig).is_some());
        assert!(cache.get_session_signature("sid-1").is_some());

        cache.clear();

        assert!(cache.get_tool_signature("tool_1").is_none());
        assert!(cache.get_signature_family(&sig).is_none());
        assert!(cache.get_session_signature("sid-1").is_none());
    }
}
