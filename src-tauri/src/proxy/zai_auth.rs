use axum::http::HeaderValue;

/// Normalize a configured z.ai API key to a raw token string.
///
/// Users sometimes paste values like `Bearer <token>` (from docs/curl examples).
/// Internally we keep configuration flexible but always emit correct headers.
pub fn normalize_api_key(raw: &str) -> String {
    let trimmed = raw.trim();
    let without_prefix = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))
        .unwrap_or(trimmed);
    without_prefix.trim().to_string()
}

pub fn bearer_header_value(raw_api_key: &str) -> Result<HeaderValue, String> {
    let token = normalize_api_key(raw_api_key);
    if token.is_empty() {
        return Err("Missing API key".to_string());
    }
    HeaderValue::from_str(&format!("Bearer {}", token))
        .map_err(|_| "Invalid API key value (cannot be used as an HTTP header)".to_string())
}

pub fn raw_header_value(raw_api_key: &str) -> Result<HeaderValue, String> {
    let token = normalize_api_key(raw_api_key);
    if token.is_empty() {
        return Err("Missing API key".to_string());
    }
    HeaderValue::from_str(&token)
        .map_err(|_| "Invalid API key value (cannot be used as an HTTP header)".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_bearer_prefix_and_whitespace() {
        assert_eq!(normalize_api_key("Bearer abc"), "abc");
        assert_eq!(normalize_api_key("bearer abc"), "abc");
        assert_eq!(normalize_api_key("  Bearer   abc  "), "abc");
        assert_eq!(normalize_api_key("abc"), "abc");
    }
}

