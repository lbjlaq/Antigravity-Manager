// thought_signature storage for Gemini 3+ tool calls.
//
// NOTE: This must be scoped by `sessionId` to avoid cross-conversation collisions when multiple
// requests are in flight (e.g. Claude Code running parallel tasks).

use dashmap::DashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const THOUGHT_SIG_TTL: Duration = Duration::from_secs(30 * 60);
const GLOBAL_SESSION_ID: &str = "global";

#[derive(Clone)]
struct ThoughtSigEntry {
    sig: String,
    updated_at: Instant,
}

static THOUGHT_SIG_STORE: OnceLock<DashMap<String, ThoughtSigEntry>> = OnceLock::new();

fn store() -> &'static DashMap<String, ThoughtSigEntry> {
    THOUGHT_SIG_STORE.get_or_init(DashMap::new)
}

fn normalized_session_id(session_id: &str) -> &str {
    let s = session_id.trim();
    if s.is_empty() {
        GLOBAL_SESSION_ID
    } else {
        s
    }
}

fn cleanup_expired(now: Instant) {
    let keys: Vec<String> = store()
        .iter()
        .filter_map(|e| {
            if now.duration_since(e.value().updated_at) > THOUGHT_SIG_TTL {
                Some(e.key().clone())
            } else {
                None
            }
        })
        .collect();

    for k in keys {
        store().remove(&k);
    }
}

/// Store thought_signature scoped to a session id.
/// Only stores if the new signature is longer than the existing one (per session),
/// to avoid short/partial signatures overwriting valid ones.
pub fn store_thought_signature_for_session(session_id: &str, sig: &str) {
    let session_id = normalized_session_id(session_id).to_string();
    let now = Instant::now();
    cleanup_expired(now);

    if sig.trim().is_empty() {
        return;
    }

    match store().get_mut(&session_id) {
        Some(mut entry) => {
            let should_store = sig.len() > entry.sig.len();
            if should_store {
                tracing::info!(
                    "[ThoughtSig] Storing new signature for session '{}' (length: {}, replacing old length: {})",
                    session_id,
                    sig.len(),
                    entry.sig.len()
                );
                entry.sig = sig.to_string();
            } else {
                tracing::debug!(
                    "[ThoughtSig] Skipping shorter signature for session '{}' (new length: {}, existing length: {})",
                    session_id,
                    sig.len(),
                    entry.sig.len()
                );
            }
            entry.updated_at = now;
        }
        None => {
            tracing::info!(
                "[ThoughtSig] Storing new signature for session '{}' (length: {})",
                session_id,
                sig.len()
            );
            store().insert(
                session_id,
                ThoughtSigEntry {
                    sig: sig.to_string(),
                    updated_at: now,
                },
            );
        }
    }
}

/// Get the stored thought_signature for a given session id without clearing it.
pub fn get_thought_signature_for_session(session_id: &str) -> Option<String> {
    let session_id = normalized_session_id(session_id).to_string();
    let now = Instant::now();
    cleanup_expired(now);

    let entry = store().get(&session_id)?;
    if now.duration_since(entry.updated_at) > THOUGHT_SIG_TTL {
        drop(entry);
        store().remove(&session_id);
        return None;
    }

    Some(entry.sig.clone())
}

#[allow(dead_code)]
pub fn clear_thought_signature_for_session(session_id: &str) {
    let session_id = normalized_session_id(session_id);
    store().remove(session_id);
}

// Backwards-compatible global API (mapped to the "global" session).
#[allow(dead_code)]
pub fn store_thought_signature(sig: &str) {
    store_thought_signature_for_session(GLOBAL_SESSION_ID, sig);
}

#[allow(dead_code)]
pub fn get_thought_signature() -> Option<String> {
    get_thought_signature_for_session(GLOBAL_SESSION_ID)
}

#[allow(dead_code)]
pub fn take_thought_signature() -> Option<String> {
    let entry = store().remove(GLOBAL_SESSION_ID)?;
    Some(entry.1.sig)
}

#[allow(dead_code)]
pub fn clear_thought_signature() {
    clear_thought_signature_for_session(GLOBAL_SESSION_ID);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_storage() {
        // Clear any existing state (global)
        clear_thought_signature();

        // Should be empty initially
        assert!(get_thought_signature().is_none());

        // Store a signature
        store_thought_signature("test_signature_1234");
        assert_eq!(
            get_thought_signature(),
            Some("test_signature_1234".to_string())
        );

        // Shorter signature should NOT overwrite
        store_thought_signature("short");
        assert_eq!(
            get_thought_signature(),
            Some("test_signature_1234".to_string())
        );

        // Longer signature SHOULD overwrite
        store_thought_signature("test_signature_1234_longer_version");
        assert_eq!(
            get_thought_signature(),
            Some("test_signature_1234_longer_version".to_string())
        );

        // Take should clear
        let taken = take_thought_signature();
        assert_eq!(
            taken,
            Some("test_signature_1234_longer_version".to_string())
        );
        assert!(get_thought_signature().is_none());
    }

    #[test]
    fn test_signature_storage_scoped_by_session() {
        clear_thought_signature_for_session("s1");
        clear_thought_signature_for_session("s2");

        store_thought_signature_for_session("s1", "sig_session_1");
        store_thought_signature_for_session("s2", "sig_session_2");

        assert_eq!(
            get_thought_signature_for_session("s1"),
            Some("sig_session_1".to_string())
        );
        assert_eq!(
            get_thought_signature_for_session("s2"),
            Some("sig_session_2".to_string())
        );

        // Shorter signature should not overwrite within the same session.
        store_thought_signature_for_session("s1", "x");
        assert_eq!(
            get_thought_signature_for_session("s1"),
            Some("sig_session_1".to_string())
        );
    }

    #[test]
    fn test_signature_ttl_cleanup() {
        let now = Instant::now();
        store().insert(
            "expired_session".to_string(),
            ThoughtSigEntry {
                sig: "expired_sig".to_string(),
                updated_at: now - THOUGHT_SIG_TTL - Duration::from_secs(1),
            },
        );

        assert!(get_thought_signature_for_session("expired_session").is_none());
    }
}
