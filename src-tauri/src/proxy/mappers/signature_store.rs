// Global thought_signature storage shared by all endpoints
// Used to capture and replay signatures for Gemini 3+ function calls when clients don't pass them back.

use std::sync::{Mutex, OnceLock};

static GLOBAL_THOUGHT_SIG: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn get_thought_sig_storage() -> &'static Mutex<Option<String>> {
    GLOBAL_THOUGHT_SIG.get_or_init(|| Mutex::new(None))
}

/// Store thought_signature to global storage.
/// Only stores if the new signature is longer than the existing one,
/// to avoid short/partial signatures overwriting valid ones.
///
/// DEPRECATED: Use SignatureCache::cache_session_signature instead for session-isolated storage.
#[allow(dead_code)] // Deprecated, kept for backward compatibility
pub fn store_thought_signature(sig: &str) {
    if let Ok(mut guard) = get_thought_sig_storage().lock() {
        let should_store = match &*guard {
            None => true,
            Some(existing) => sig.len() > existing.len(),
        };

        if should_store {
            tracing::debug!(
                "[ThoughtSig] Storing new signature (length: {}, replacing old length: {:?})",
                sig.len(),
                guard.as_ref().map(|s| s.len())
            );
            *guard = Some(sig.to_string());
        } else {
            tracing::debug!(
                "[ThoughtSig] Skipping shorter signature (new length: {}, existing length: {})",
                sig.len(),
                guard.as_ref().map(|s| s.len()).unwrap_or(0)
            );
        }
    }
}

/// Get the stored thought_signature without clearing it.
pub fn get_thought_signature() -> Option<String> {
    if let Ok(guard) = get_thought_sig_storage().lock() {
        guard.clone()
    } else {
        None
    }
}

/// Get and clear the stored thought_signature.
#[allow(dead_code)]
pub fn take_thought_signature() -> Option<String> {
    if let Ok(mut guard) = get_thought_sig_storage().lock() {
        guard.take()
    } else {
        None
    }
}

/// Clear the stored thought_signature.
#[allow(dead_code)]
pub fn clear_thought_signature() {
    if let Ok(mut guard) = get_thought_sig_storage().lock() {
        *guard = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_storage() {
        // Clear any existing state
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
}
