pub fn mask_email(email: &str) -> String {
    let (local, domain) = match email.split_once('@') {
        Some(v) => v,
        None => return "<invalid-email>".to_string(),
    };

    let local_masked = match local.chars().count() {
        0 => "***".to_string(),
        1 => format!("{}***", local),
        2 => {
            let mut chars = local.chars();
            format!("{}{}***", chars.next().unwrap_or('*'), chars.next().unwrap_or('*'))
        }
        _ => {
            let first = local.chars().next().unwrap_or('*');
            let last = local.chars().rev().next().unwrap_or('*');
            format!("{first}***{last}")
        }
    };

    let domain_masked = match domain.split_once('.') {
        Some((root, tld)) => {
            let root_masked = match root.chars().count() {
                0 => "***".to_string(),
                1 => format!("{}***", root),
                _ => {
                    let first = root.chars().next().unwrap_or('*');
                    format!("{first}***")
                }
            };
            format!("{root_masked}.{tld}")
        }
        None => "***".to_string(),
    };

    format!("{local_masked}@{domain_masked}")
}

/// Anonymize a stable identifier for display/logging without leaking the full value.
/// Example: `abcd1234efgh5678` -> `abcd…5678`.
pub fn anonymize_id(id: &str) -> String {
    let s = id.trim();
    if s.is_empty() {
        return "<empty>".to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= 8 {
        return "<redacted>".to_string();
    }
    let start: String = chars.iter().take(4).collect();
    let end: String = chars.iter().skip(chars.len() - 4).collect();
    format!("{start}…{end}")
}

/// ASCII-safe variant of `anonymize_id`, suitable for HTTP header values.
/// Example: `abcd1234efgh5678` -> `abcd...5678`.
pub fn anonymize_id_ascii(id: &str) -> String {
    let s = id.trim();
    if s.is_empty() {
        return "<empty>".to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= 8 {
        return "<redacted>".to_string();
    }
    let start: String = chars.iter().take(4).collect();
    let end: String = chars.iter().skip(chars.len() - 4).collect();
    format!("{start}...{end}")
}

/// Stable SHA-256 hex digest for correlation without leaking the original value.
pub fn stable_hash_hex(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(value.as_bytes());
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}
