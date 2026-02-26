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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_session_id() {
        let x = derive_session_id("my_account@gmail.com");
        let y = derive_session_id("my_account@gmail.com");
        assert_eq!(x, y);
    }
}
