// [NEW v4.1.24] Tools for deriving stable session identifiers

/// From account ID string to a stable negative signed integer session ID
/// Implements FNV-1a hash which matches the official client behavior of sending
/// a large negative integer for `sessionId`.
pub fn derive_session_id(account_id: &str) -> String {
    let mut hash: i64 = -3750763034362895579_i64; // FNV offset basis
    for byte in account_id.bytes() {
        hash = hash.wrapping_mul(1099511628211_i64);
        hash ^= byte as i64;
    }
    hash.to_string()
}

// [FIX session-1M] Upstream accumulates conversation input server-side per sessionId.
// A session that drives many tool loops can push the accumulated input past 1M tokens,
// after which EVERY request with the same sessionId fails with
// 400 "The input token count exceeds the maximum number of tokens allowed 1048576"
// until the upstream session expires. Bumping the sessionId generation forces the
// upstream to start a fresh session, transparently recovering the conversation.

const SESSION_BUMP_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
const SESSION_BUMP_MAX_ENTRIES: usize = 4096;

/// Monotonic generation counter per (account_id, conversation fingerprint) with TTL & cap (Issue #4054).
static SESSION_BUMPS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, (u64, std::time::Instant)>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn bump_key(account_id: &str, fingerprint: &str) -> String {
    format!("{}::{}", account_id, fingerprint)
}

fn evict_session_bumps_if_needed(
    map: &mut std::collections::HashMap<String, (u64, std::time::Instant)>,
) {
    if map.len() <= SESSION_BUMP_MAX_ENTRIES {
        return;
    }
    map.retain(|_, (_, ts)| ts.elapsed() < SESSION_BUMP_TTL);
    if map.len() > SESSION_BUMP_MAX_ENTRIES {
        let target = SESSION_BUMP_MAX_ENTRIES * 4 / 5;
        let remove_count = map.len().saturating_sub(target);
        let mut by_age: Vec<(String, std::time::Instant)> =
            map.iter().map(|(k, (_, ts))| (k.clone(), *ts)).collect();
        by_age.sort_unstable_by_key(|(_, ts)| *ts);
        for (k, _) in by_age.into_iter().take(remove_count) {
            map.remove(&k);
        }
    }
}

pub fn session_bumps_len() -> usize {
    SESSION_BUMPS.lock().map(|m| m.len()).unwrap_or(0)
}

/// Current sessionId generation for (account, conversation). Starts at 0.
pub fn current_bump(account_id: &str, fingerprint: &str) -> u64 {
    if let Ok(map) = SESSION_BUMPS.lock() {
        map.get(&bump_key(account_id, fingerprint))
            .filter(|(_, ts)| ts.elapsed() < SESSION_BUMP_TTL)
            .map(|(gen, _)| *gen)
            .unwrap_or(0)
    } else {
        0
    }
}

/// Advance the sessionId generation for (account, conversation) and return the new value.
pub fn bump_session(account_id: &str, fingerprint: &str) -> u64 {
    if let Ok(mut map) = SESSION_BUMPS.lock() {
        let now = std::time::Instant::now();
        let entry = map
            .entry(bump_key(account_id, fingerprint))
            .or_insert((0, now));
        if entry.1.elapsed() >= SESSION_BUMP_TTL {
            entry.0 = 0;
        }
        entry.0 += 1;
        entry.1 = now;
        let counter = entry.0;
        evict_session_bumps_if_needed(&mut map);
        tracing::warn!(
            "[Session] Bumped sessionId generation to {} for account {} fingerprint {}",
            counter,
            &account_id[..account_id.len().min(8)],
            &fingerprint[..fingerprint.len().min(16)]
        );
        counter
    } else {
        0
    }
}

/// Derive the upstream sessionId for (account, conversation fingerprint, generation).
/// Stable within one conversation generation (keeps upstream server-side cache hits),
/// but distinct across conversations and after a bump.
pub fn derive_session_scoped(account_id: &str, fingerprint: &str, generation: u64) -> String {
    if fingerprint.is_empty() && generation == 0 {
        return derive_session_id(account_id);
    }
    derive_session_id(&format!("{}|{}|{}", account_id, fingerprint, generation))
}

/// 上游 `sessionId` 跟思维库的 `store_key` 走。账号粘性是另一把键。
pub fn apply_upstream_session(inner: &mut serde_json::Value, account_id: &str, store_key: &str) {
    let generation = current_bump(account_id, store_key);
    inner["sessionId"] =
        serde_json::json!(derive_session_scoped(account_id, store_key, generation));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_session_id() {
        let x = derive_session_id("my_account@gmail.com");
        let y = derive_session_id("my_account@gmail.com");
        assert_eq!(x, y);
    }

    #[test]
    fn test_bump_isolated_per_account_and_fingerprint() {
        let a1 = current_bump("acc1", "fp1");
        let a2 = current_bump("acc2", "fp1");
        let a3 = current_bump("acc1", "fp2");
        assert_eq!(a1, 0);
        assert_eq!(a2, 0);
        assert_eq!(a3, 0);

        assert_eq!(bump_session("acc1", "fp1"), 1);
        assert_eq!(current_bump("acc1", "fp1"), 1);
        // other keys untouched
        assert_eq!(current_bump("acc2", "fp1"), 0);
        assert_eq!(current_bump("acc1", "fp2"), 0);

        assert_eq!(bump_session("acc1", "fp1"), 2);
        assert_eq!(current_bump("acc1", "fp1"), 2);
    }

    #[test]
    fn test_issue_4054_session_bumps_bounded_capacity() {
        let mut map = std::collections::HashMap::new();
        let now = std::time::Instant::now();
        for i in 0..(SESSION_BUMP_MAX_ENTRIES + 500) {
            map.insert(format!("acc::fp_{i}"), (1, now));
        }
        evict_session_bumps_if_needed(&mut map);
        assert!(
            map.len() <= SESSION_BUMP_MAX_ENTRIES,
            "session bumps exceeded cap: {}",
            map.len()
        );
    }

    #[test]
    fn test_derive_session_scoped() {
        // Generation 0 with empty fingerprint keeps legacy behavior
        assert_eq!(
            derive_session_scoped("acc", "", 0),
            derive_session_id("acc")
        );
        // Same inputs -> stable
        assert_eq!(
            derive_session_scoped("acc", "fp", 0),
            derive_session_scoped("acc", "fp", 0)
        );
        // Different fingerprint or generation -> distinct sessions
        assert_ne!(
            derive_session_scoped("acc", "fp1", 0),
            derive_session_scoped("acc", "fp2", 0)
        );
        assert_ne!(
            derive_session_scoped("acc", "fp", 0),
            derive_session_scoped("acc", "fp", 1)
        );
        // Still a valid signed-integer string (official client format)
        assert!(derive_session_scoped("acc", "fp", 1).parse::<i64>().is_ok());
    }
}
