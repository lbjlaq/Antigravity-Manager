//! Error handling utilities for Gemini 3 Pro Image generation (Story-007-03)
//!
//! This module provides:
//! - Error categorization for structured logging
//! - Prompt hashing for privacy (no PII in logs)
//! - User-friendly error messages with resolution suggestions
//! - Error code mapping for troubleshooting

use sha2::{Digest, Sha256};

/// Error categories for structured logging
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Client-side validation errors (400 Bad Request, invalid parameters)
    UserError,
    /// Upstream API errors (429 Rate Limit, 503 Service Unavailable, quota issues)
    ApiError,
    /// Internal server errors (500 Internal Server Error, unexpected exceptions)
    SystemError,
    /// Network errors (timeouts, connection failures)
    NetworkError,
}

impl ErrorCategory {
    /// Convert to string for logging
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCategory::UserError => "USER_ERROR",
            ErrorCategory::ApiError => "API_ERROR",
            ErrorCategory::SystemError => "SYSTEM_ERROR",
            ErrorCategory::NetworkError => "NETWORK_ERROR",
        }
    }
}

/// Categorize error based on HTTP status code and error message
///
/// # Arguments
/// * `status_code` - HTTP status code from upstream API
/// * `error_text` - Error message text from response
///
/// # Returns
/// Error category for structured logging
///
/// # Examples
/// ```
/// let category = categorize_error(429, "Rate limit exceeded");
/// assert_eq!(category, ErrorCategory::ApiError);
///
/// let category = categorize_error(400, "Missing prompt field");
/// assert_eq!(category, ErrorCategory::UserError);
/// ```
pub fn categorize_error(status_code: u16, error_text: &str) -> ErrorCategory {
    let error_lower = error_text.to_lowercase();

    match status_code {
        // Client errors (4xx)
        400 => ErrorCategory::UserError,

        // Auth errors - quota is API error, others are user errors
        401 | 403 => {
            if error_lower.contains("quota")
                || error_lower.contains("rate")
                || error_lower.contains("limit")
            {
                ErrorCategory::ApiError
            } else {
                ErrorCategory::UserError
            }
        }

        // Rate limiting and quota - always API errors
        429 => ErrorCategory::ApiError,

        // Server errors (5xx)
        500..=599 => {
            // Distinguish between API service issues and internal system errors
            if error_lower.contains("upstream")
                || error_lower.contains("gemini")
                || error_lower.contains("google")
            {
                ErrorCategory::ApiError
            } else {
                ErrorCategory::SystemError
            }
        }

        // Network-related errors
        _ => {
            if error_lower.contains("timeout")
                || error_lower.contains("connection")
                || error_lower.contains("network")
                || error_lower.contains("unreachable")
            {
                ErrorCategory::NetworkError
            } else {
                ErrorCategory::SystemError
            }
        }
    }
}

/// Hash prompt for privacy-preserving logging
///
/// Uses SHA256 and returns first 16 characters of hex digest.
/// This allows correlation of errors for the same prompt without exposing PII.
///
/// # Arguments
/// * `prompt` - User prompt text to hash
///
/// # Returns
/// First 16 characters of SHA256 hex digest
///
/// # Examples
/// ```
/// let hash = hash_prompt("A beautiful sunset over mountains");
/// assert_eq!(hash.len(), 16);
/// ```
pub fn hash_prompt(prompt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prompt.as_bytes());
    let result = hasher.finalize();
    // First 16 characters of hex digest
    format!("{:x}", result)[..16].to_string()
}

/// Format user-friendly error message with resolution suggestions
///
/// # Arguments
/// * `category` - Error category
/// * `status_code` - HTTP status code
/// * `details` - Error details from upstream
///
/// # Returns
/// User-friendly error message with resolution suggestions
pub fn format_error_message(category: ErrorCategory, status_code: u16, details: &str) -> String {
    let details_lower = details.to_lowercase();

    match category {
        ErrorCategory::UserError => {
            if status_code == 400 {
                if details_lower.contains("prompt") {
                    format!(
                        "Invalid request: Missing or empty prompt. Please provide a text description for image generation. Details: {}",
                        details
                    )
                } else if details_lower.contains("size") || details_lower.contains("dimension") {
                    format!(
                        "Invalid request: Unsupported image size. Supported sizes: 1024x1024, 1920x1080, 2560x1080, etc. Details: {}",
                        details
                    )
                } else {
                    format!(
                        "Invalid request: Please check your parameters (prompt, size, quality, style). Details: {}",
                        details
                    )
                }
            } else {
                format!("Client error ({}): {}", status_code, details)
            }
        }

        ErrorCategory::ApiError => {
            if status_code == 429
                || details_lower.contains("quota")
                || details_lower.contains("rate")
            {
                format!(
                    "Rate limit exceeded: The API quota for image generation has been exhausted for this account. The system will automatically rotate to another account. If all accounts are exhausted, please wait for quota reset (typically 24 hours). Details: {}",
                    details
                )
            } else if details_lower.contains("safety")
                || details_lower.contains("filter")
                || details_lower.contains("blocked")
            {
                format!(
                    "Content safety filter triggered: The generated image was blocked by content safety filters. Try adjusting your prompt or lowering the safety_threshold setting. Current threshold can be configured via GEMINI_IMAGE_SAFETY_THRESHOLD environment variable (OFF|LOW|MEDIUM|HIGH) or per-request safety_threshold parameter. Details: {}",
                    details
                )
            } else if status_code == 503 {
                format!(
                    "Service unavailable: The Gemini API is temporarily unavailable or all accounts are rate-limited. The system will retry automatically. Details: {}",
                    details
                )
            } else {
                format!(
                    "API error ({}): Upstream Gemini API returned an error. The system may retry with a different account. Details: {}",
                    status_code, details
                )
            }
        }

        ErrorCategory::SystemError => {
            format!(
                "Internal server error ({}): An unexpected error occurred. Please contact support if this persists. Details: {}",
                status_code, details
            )
        }

        ErrorCategory::NetworkError => {
            format!(
                "Network error: Unable to connect to upstream API. This may be a temporary connectivity issue. The system will retry automatically. Details: {}",
                details
            )
        }
    }
}

/// Error code reference for troubleshooting
///
/// Maps common error patterns to error codes and resolution steps
///
/// NOTE: This is a utility struct for documentation purposes and future integration
/// with monitoring systems. Currently not used in runtime logging but provides
/// standardized error codes for the troubleshooting guide.
#[allow(dead_code)]
pub struct ErrorCodeReference {
    pub code: &'static str,
    pub description: &'static str,
    pub resolution: &'static str,
}

/// Get error code reference for common scenarios
///
/// NOTE: This is a utility function for documentation purposes and future integration
/// with monitoring systems. Currently not used in runtime logging but provides
/// error code lookups for troubleshooting guides and monitoring dashboards.
#[allow(dead_code)]
pub fn get_error_reference(status_code: u16, error_text: &str) -> Option<ErrorCodeReference> {
    let error_lower = error_text.to_lowercase();

    if status_code == 429 || error_lower.contains("quota_exhausted") {
        return Some(ErrorCodeReference {
            code: "IMG_QUOTA_EXHAUSTED",
            description: "Image generation quota exhausted for current account",
            resolution: "System will automatically rotate to next available account. If all accounts exhausted, quota resets in 24 hours. Consider adding more Google accounts or reducing image generation rate.",
        });
    }

    if error_lower.contains("safety")
        || error_lower.contains("filter")
        || error_lower.contains("blocked")
    {
        return Some(ErrorCodeReference {
            code: "IMG_SAFETY_BLOCKED",
            description: "Content safety filter blocked image generation",
            resolution: "Adjust prompt to avoid potentially unsafe content, or lower safety threshold via GEMINI_IMAGE_SAFETY_THRESHOLD env var (OFF|LOW|MEDIUM|HIGH) or safety_threshold request parameter. Current default is OFF for backward compatibility.",
        });
    }

    if status_code == 400 && error_lower.contains("prompt") {
        return Some(ErrorCodeReference {
            code: "IMG_MISSING_PROMPT",
            description: "Request missing required 'prompt' field",
            resolution: "Ensure your request includes a 'prompt' field with text description of desired image.",
        });
    }

    if status_code == 503 {
        return Some(ErrorCodeReference {
            code: "IMG_SERVICE_UNAVAILABLE",
            description: "Gemini API temporarily unavailable or all accounts rate-limited",
            resolution: "System will retry automatically with exponential backoff. If issue persists, check Gemini API status or add more accounts.",
        });
    }

    if error_lower.contains("timeout") || error_lower.contains("connection") {
        return Some(ErrorCodeReference {
            code: "IMG_NETWORK_ERROR",
            description: "Network connectivity issue to upstream Gemini API",
            resolution: "System will retry automatically. Check network connectivity and firewall settings if issue persists.",
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================================================================================
    // TEST SUITE: Error Categorization
    // ==================================================================================

    #[test]
    fn test_categorize_user_error_400() {
        let category = categorize_error(400, "Missing 'prompt' field");
        assert_eq!(category, ErrorCategory::UserError);
    }

    #[test]
    fn test_categorize_api_error_429() {
        let category = categorize_error(429, "Rate limit exceeded");
        assert_eq!(category, ErrorCategory::ApiError);
    }

    #[test]
    fn test_categorize_api_error_quota() {
        // 403 with quota = API error
        let category = categorize_error(403, "QUOTA_EXHAUSTED for image generation");
        assert_eq!(category, ErrorCategory::ApiError);

        // 401 with rate limit = API error
        let category = categorize_error(401, "Rate limit exceeded");
        assert_eq!(category, ErrorCategory::ApiError);
    }

    #[test]
    fn test_categorize_user_error_auth() {
        // 403 without quota = User error
        let category = categorize_error(403, "Invalid API key");
        assert_eq!(category, ErrorCategory::UserError);

        // 401 without quota/rate = User error
        let category = categorize_error(401, "Authentication failed");
        assert_eq!(category, ErrorCategory::UserError);
    }

    #[test]
    fn test_categorize_system_error_500() {
        let category = categorize_error(500, "Internal server error");
        assert_eq!(category, ErrorCategory::SystemError);
    }

    #[test]
    fn test_categorize_api_error_upstream_500() {
        // 500 with upstream/gemini/google = API error
        let category = categorize_error(500, "Upstream Gemini API error");
        assert_eq!(category, ErrorCategory::ApiError);

        let category = categorize_error(503, "Google service unavailable");
        assert_eq!(category, ErrorCategory::ApiError);
    }

    #[test]
    fn test_categorize_network_error() {
        let category = categorize_error(0, "Connection timeout");
        assert_eq!(category, ErrorCategory::NetworkError);

        let category = categorize_error(0, "Network unreachable");
        assert_eq!(category, ErrorCategory::NetworkError);
    }

    // ==================================================================================
    // TEST SUITE: Prompt Hashing
    // ==================================================================================

    #[test]
    fn test_hash_prompt_length() {
        let hash = hash_prompt("A beautiful sunset over mountains");
        assert_eq!(hash.len(), 16, "Hash should be exactly 16 characters");
    }

    #[test]
    fn test_hash_prompt_deterministic() {
        let prompt = "A serene mountain landscape at sunset";
        let hash1 = hash_prompt(prompt);
        let hash2 = hash_prompt(prompt);
        assert_eq!(hash1, hash2, "Same prompt should produce same hash");
    }

    #[test]
    fn test_hash_prompt_different() {
        let hash1 = hash_prompt("A serene mountain landscape");
        let hash2 = hash_prompt("A beautiful sunset");
        assert_ne!(
            hash1, hash2,
            "Different prompts should produce different hashes"
        );
    }

    #[test]
    fn test_hash_prompt_empty() {
        let hash = hash_prompt("");
        assert_eq!(
            hash.len(),
            16,
            "Empty prompt should still produce 16-char hash"
        );
    }

    #[test]
    fn test_hash_prompt_special_chars() {
        let hash = hash_prompt("Test with émojis 🎨 and spëcial çhars!");
        assert_eq!(
            hash.len(),
            16,
            "Special characters should be handled correctly"
        );
    }

    // ==================================================================================
    // TEST SUITE: User-Friendly Error Messages
    // ==================================================================================

    #[test]
    fn test_format_error_missing_prompt() {
        let msg = format_error_message(ErrorCategory::UserError, 400, "Missing 'prompt' field");
        assert!(msg.contains("Missing or empty prompt"));
        assert!(msg.contains("provide a text description"));
    }

    #[test]
    fn test_format_error_quota_exhausted() {
        let msg = format_error_message(ErrorCategory::ApiError, 429, "QUOTA_EXHAUSTED");
        assert!(msg.contains("Rate limit exceeded"));
        assert!(msg.contains("automatically rotate"));
        assert!(msg.contains("24 hours"));
    }

    #[test]
    fn test_format_error_safety_filter() {
        let msg = format_error_message(
            ErrorCategory::ApiError,
            400,
            "Content blocked by safety filter",
        );
        assert!(msg.contains("safety filter"));
        assert!(msg.contains("safety_threshold"));
        assert!(msg.contains("GEMINI_IMAGE_SAFETY_THRESHOLD"));
    }

    #[test]
    fn test_format_error_service_unavailable() {
        let msg = format_error_message(
            ErrorCategory::ApiError,
            503,
            "Service temporarily unavailable",
        );
        assert!(msg.contains("Service unavailable"));
        assert!(msg.contains("retry automatically"));
    }

    #[test]
    fn test_format_error_network() {
        let msg = format_error_message(ErrorCategory::NetworkError, 0, "Connection timeout");
        assert!(msg.contains("Network error"));
        assert!(msg.contains("connectivity issue"));
        assert!(msg.contains("retry automatically"));
    }

    // ==================================================================================
    // TEST SUITE: Error Code Reference
    // ==================================================================================

    #[test]
    fn test_error_reference_quota() {
        let reference = get_error_reference(429, "QUOTA_EXHAUSTED").unwrap();
        assert_eq!(reference.code, "IMG_QUOTA_EXHAUSTED");
        assert!(reference.description.contains("quota exhausted"));
        assert!(reference.resolution.contains("rotate"));
    }

    #[test]
    fn test_error_reference_safety() {
        let reference = get_error_reference(400, "Content blocked by safety filter").unwrap();
        assert_eq!(reference.code, "IMG_SAFETY_BLOCKED");
        assert!(reference.description.contains("safety filter"));
        assert!(reference.resolution.contains("safety_threshold"));
    }

    #[test]
    fn test_error_reference_missing_prompt() {
        let reference = get_error_reference(400, "Missing 'prompt' field").unwrap();
        assert_eq!(reference.code, "IMG_MISSING_PROMPT");
        assert!(reference.description.contains("prompt"));
    }

    #[test]
    fn test_error_reference_none() {
        let reference = get_error_reference(418, "I'm a teapot");
        assert!(reference.is_none(), "Unknown errors should return None");
    }

    // ==================================================================================
    // TEST SUITE: ErrorCategory String Conversion
    // ==================================================================================

    #[test]
    fn test_error_category_as_str() {
        assert_eq!(ErrorCategory::UserError.as_str(), "USER_ERROR");
        assert_eq!(ErrorCategory::ApiError.as_str(), "API_ERROR");
        assert_eq!(ErrorCategory::SystemError.as_str(), "SYSTEM_ERROR");
        assert_eq!(ErrorCategory::NetworkError.as_str(), "NETWORK_ERROR");
    }
}
