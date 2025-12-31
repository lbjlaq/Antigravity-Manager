// thoughtSignature storage for Gemini 3+ tool calls.
//
// Design goals:
// - Prevent cross-conversation collisions: key by `sessionId:tool_use_id`
// - Graceful degradation: if sessionId/tool_use_id missing -> No-Op
// - Avoid partial overwrite: prefer longer signatures (streaming deltas)

use dashmap::DashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const THOUGHT_SIG_TTL: Duration = Duration::from_secs(2 * 60 * 60); // 2 hours
const MIN_THOUGHT_SIGNATURE_LEN: usize = 50;

#[derive(Clone)]
struct ThoughtSigEntry {
    sig: String,
    updated_at: Instant,
}

static SESSION_SIG_STORE: OnceLock<DashMap<String, ThoughtSigEntry>> = OnceLock::new();
static TOOL_SIG_STORE: OnceLock<DashMap<String, ThoughtSigEntry>> = OnceLock::new();

fn session_store() -> &'static DashMap<String, ThoughtSigEntry> {
    SESSION_SIG_STORE.get_or_init(DashMap::new)
}

fn tool_store() -> &'static DashMap<String, ThoughtSigEntry> {
    TOOL_SIG_STORE.get_or_init(DashMap::new)
}

fn cleanup_expired_in_store(store: &DashMap<String, ThoughtSigEntry>, now: Instant) {
    let keys: Vec<String> = store
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
        store.remove(&k);
    }
}

/// Store thought_signature scoped to a session id.
/// Only stores if the new signature is longer than the existing one (per session),
/// to avoid short/partial signatures overwriting valid ones.
pub fn store_thought_signature_for_session(session_id: &str, sig: &str) {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return;
    }
    let now = Instant::now();
    cleanup_expired_in_store(session_store(), now);

    let sig = sig.trim();
    if sig.is_empty() || sig.len() < MIN_THOUGHT_SIGNATURE_LEN {
        return;
    }

    match session_store().get_mut(session_id) {
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
            session_store().insert(
                session_id.to_string(),
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
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return None;
    }
    let now = Instant::now();
    cleanup_expired_in_store(session_store(), now);

    let entry = session_store().get(session_id)?;
    if now.duration_since(entry.updated_at) > THOUGHT_SIG_TTL {
        drop(entry);
        session_store().remove(session_id);
        return None;
    }

    Some(entry.sig.clone())
}

#[allow(dead_code)]
pub fn clear_thought_signature_for_session(session_id: &str) {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return;
    }
    session_store().remove(session_id);
}

fn tool_key(session_id: &str, tool_use_id: &str) -> Option<String> {
    let sid = session_id.trim();
    let tid = tool_use_id.trim();
    if sid.is_empty() || tid.is_empty() {
        return None;
    }
    Some(format!("{}:{}", sid, tid))
}

/// Store thoughtSignature scoped to `sessionId:tool_use_id`.
/// Graceful degradation: if sessionId/tool_use_id missing -> No-Op.
pub fn store_thought_signature_for_tool(session_id: &str, tool_use_id: &str, sig: &str) {
    let Some(key) = tool_key(session_id, tool_use_id) else {
        return;
    };

    let sig = sig.trim();
    if sig.is_empty() || sig.len() < MIN_THOUGHT_SIGNATURE_LEN {
        return;
    }

    let now = Instant::now();
    cleanup_expired_in_store(tool_store(), now);

    match tool_store().get_mut(&key) {
        Some(mut entry) => {
            let should_store = sig.len() > entry.sig.len();
            if should_store {
                tracing::info!(
                    "[ThoughtSig] Storing new tool signature for '{}' (length: {}, replacing old length: {})",
                    key,
                    sig.len(),
                    entry.sig.len()
                );
                entry.sig = sig.to_string();
            } else {
                tracing::debug!(
                    "[ThoughtSig] Skipping shorter tool signature for '{}' (new length: {}, existing length: {})",
                    key,
                    sig.len(),
                    entry.sig.len()
                );
            }
            entry.updated_at = now;
        }
        None => {
            tracing::info!(
                "[ThoughtSig] Storing new tool signature for '{}' (length: {})",
                key,
                sig.len()
            );
            tool_store().insert(
                key,
                ThoughtSigEntry {
                    sig: sig.to_string(),
                    updated_at: now,
                },
            );
        }
    }
}

/// Restore thoughtSignature for a specific tool use.
/// Graceful degradation: if sessionId/tool_use_id missing -> None.
pub fn get_thought_signature_for_tool(session_id: &str, tool_use_id: &str) -> Option<String> {
    let key = tool_key(session_id, tool_use_id)?;
    let now = Instant::now();
    cleanup_expired_in_store(tool_store(), now);

    let entry = tool_store().get(&key)?;
    if now.duration_since(entry.updated_at) > THOUGHT_SIG_TTL {
        drop(entry);
        tool_store().remove(&key);
        return None;
    }

    Some(entry.sig.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(len: usize, ch: char) -> String {
        std::iter::repeat(ch).take(len).collect()
    }

    #[test]
    fn test_signature_storage_scoped_by_session() {
        let s1 = "test_signature_storage_scoped_by_session:s1";
        let s2 = "test_signature_storage_scoped_by_session:s2";
        clear_thought_signature_for_session(s1);
        clear_thought_signature_for_session(s2);

        let sig1 = sig(60, 'a');
        let sig2 = sig(60, 'b');
        store_thought_signature_for_session(s1, &sig1);
        store_thought_signature_for_session(s2, &sig2);

        assert_eq!(
            get_thought_signature_for_session(s1),
            Some(sig1.clone())
        );
        assert_eq!(
            get_thought_signature_for_session(s2),
            Some(sig2.clone())
        );

        // Shorter signature should not overwrite within the same session.
        store_thought_signature_for_session(s1, "x");
        assert_eq!(
            get_thought_signature_for_session(s1),
            Some(sig1.clone())
        );
    }

    #[test]
    fn test_signature_session_ttl_cleanup() {
        let now = Instant::now();
        let key = "test_signature_session_ttl_cleanup:expired_session";
        session_store().insert(
            key.to_string(),
            ThoughtSigEntry {
                sig: "expired_sig".to_string(),
                updated_at: now - THOUGHT_SIG_TTL - Duration::from_secs(1),
            },
        );

        assert!(get_thought_signature_for_session(key).is_none());
    }

    #[test]
    fn test_tool_signature_no_op_without_session_or_tool_id() {
        store_thought_signature_for_tool("", "t1", "sig");
        store_thought_signature_for_tool("s1", "", "sig");
        assert!(get_thought_signature_for_tool("", "t1").is_none());
        assert!(get_thought_signature_for_tool("s1", "").is_none());
    }

    #[test]
    fn test_tool_signature_scoped_by_session_and_tool_id() {
        let s1 = "test_tool_signature_scoped_by_session_and_tool_id:s1";
        let s2 = "test_tool_signature_scoped_by_session_and_tool_id:s2";
        let sig1 = sig(60, 'c');
        let sig2 = sig(60, 'd');
        let sig3 = sig(60, 'e');
        store_thought_signature_for_tool(s1, "t1", &sig1);
        store_thought_signature_for_tool(s1, "t2", &sig2);
        store_thought_signature_for_tool(s2, "t1", &sig3);

        assert_eq!(
            get_thought_signature_for_tool(s1, "t1"),
            Some(sig1.clone())
        );
        assert_eq!(
            get_thought_signature_for_tool(s1, "t2"),
            Some(sig2.clone())
        );
        assert_eq!(
            get_thought_signature_for_tool(s2, "t1"),
            Some(sig3.clone())
        );
    }

    #[test]
    fn test_tool_signature_prefers_longer_value() {
        let s1 = "test_tool_signature_prefers_longer_value:s1";
        let long_sig = sig(80, 'f');
        store_thought_signature_for_tool(s1, "t1", &long_sig);
        store_thought_signature_for_tool(s1, "t1", "x");
        assert_eq!(
            get_thought_signature_for_tool(s1, "t1"),
            Some(long_sig.clone())
        );
    }

    #[test]
    fn test_tool_signature_ttl_cleanup() {
        let now = Instant::now();
        let sid = "test_tool_signature_ttl_cleanup:s1";
        let tid = "t1";
        let key = format!("{}:{}", sid, tid);
        tool_store().insert(
            key,
            ThoughtSigEntry {
                sig: "expired_sig".to_string(),
                updated_at: now - THOUGHT_SIG_TTL - Duration::from_secs(1),
            },
        );

        assert!(get_thought_signature_for_tool(sid, tid).is_none());
    }
}
