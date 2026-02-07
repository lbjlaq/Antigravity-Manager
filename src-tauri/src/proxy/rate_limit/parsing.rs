// File: src-tauri/src/proxy/rate_limit/parsing.rs
//! Parsing utilities for rate limit responses.

use regex::Regex;
use super::types::RateLimitReason;

/// Parse rate limit reason from error body
pub fn parse_rate_limit_reason(body: &str) -> RateLimitReason {
    // Try to extract reason field from JSON
    let trimmed = body.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(reason_str) = json
                .get("error")
                .and_then(|e| e.get("details"))
                .and_then(|d| d.as_array())
                .and_then(|a| a.get(0))
                .and_then(|o| o.get("reason"))
                .and_then(|v| v.as_str())
            {
                return match reason_str {
                    "QUOTA_EXHAUSTED" => RateLimitReason::QuotaExhausted,
                    "RATE_LIMIT_EXCEEDED" => RateLimitReason::RateLimitExceeded,
                    "MODEL_CAPACITY_EXHAUSTED" => RateLimitReason::ModelCapacityExhausted,
                    _ => RateLimitReason::Unknown,
                };
            }
            // Try to match from message field (fallback)
            if let Some(msg) = json
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|v| v.as_str())
            {
                let msg_lower = msg.to_lowercase();
                if msg_lower.contains("per minute") || msg_lower.contains("rate limit") {
                    return RateLimitReason::RateLimitExceeded;
                }
            }
        }
    }

    // If JSON parsing fails, try text matching
    let body_lower = body.to_lowercase();
    // Prioritize per-minute limits to avoid misclassifying TPM as Quota
    if body_lower.contains("per minute")
        || body_lower.contains("rate limit")
        || body_lower.contains("too many requests")
    {
        RateLimitReason::RateLimitExceeded
    } else if body_lower.contains("exhausted") || body_lower.contains("quota") {
        RateLimitReason::QuotaExhausted
    } else {
        RateLimitReason::Unknown
    }
}

/// Generic duration string parser: supports "2h1m1s", "42s", "500ms", etc.
pub fn parse_duration_string(s: &str) -> Option<u64> {
    tracing::debug!("[Time Parse] Attempting to parse: '{}'", s);

    // Regex to extract hours, minutes, seconds, milliseconds
    // Supports: "2h1m1s", "1h30m", "5m", "30s", "500ms", "510.790006ms"
    let re = Regex::new(r"(?:(\d+)h)?(?:(\d+)m)?(?:(\d+(?:\.\d+)?)s)?(?:(\d+(?:\.\d+)?)ms)?")
        .ok()?;
    let caps = match re.captures(s) {
        Some(c) => c,
        None => {
            tracing::warn!("[Time Parse] Regex did not match: '{}'", s);
            return None;
        }
    };

    let hours = caps
        .get(1)
        .and_then(|m| m.as_str().parse::<u64>().ok())
        .unwrap_or(0);
    let minutes = caps
        .get(2)
        .and_then(|m| m.as_str().parse::<u64>().ok())
        .unwrap_or(0);
    let seconds = caps
        .get(3)
        .and_then(|m| m.as_str().parse::<f64>().ok())
        .unwrap_or(0.0);
    let milliseconds = caps
        .get(4)
        .and_then(|m| m.as_str().parse::<f64>().ok())
        .unwrap_or(0.0);

    tracing::debug!(
        "[Time Parse] Extracted: {}h {}m {:.3}s {:.3}ms",
        hours,
        minutes,
        seconds,
        milliseconds
    );

    // Calculate total seconds, rounding up milliseconds
    let total_seconds = hours * 3600
        + minutes * 60
        + seconds.ceil() as u64
        + (milliseconds / 1000.0).ceil() as u64;

    if total_seconds == 0 {
        tracing::warn!("[Time Parse] Failed: '{}' (total seconds = 0)", s);
        None
    } else {
        tracing::info!(
            "[Time Parse] ✓ Success: '{}' => {}s ({}h {}m {:.1}s {:.1}ms)",
            s,
            total_seconds,
            hours,
            minutes,
            seconds,
            milliseconds
        );
        Some(total_seconds)
    }
}

/// Parse retry time from error response body
pub fn parse_retry_time_from_body(body: &str) -> Option<u64> {
    // A. Prefer JSON precise parsing
    let trimmed = body.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
            // 1. Google's quotaResetDelay format
            if let Some(delay_str) = json
                .get("error")
                .and_then(|e| e.get("details"))
                .and_then(|d| d.as_array())
                .and_then(|a| a.get(0))
                .and_then(|o| o.get("metadata"))
                .and_then(|m| m.get("quotaResetDelay"))
                .and_then(|v| v.as_str())
            {
                tracing::debug!("[JSON Parse] Found quotaResetDelay: '{}'", delay_str);
                if let Some(seconds) = parse_duration_string(delay_str) {
                    return Some(seconds);
                }
            }

            // 2. OpenAI's retry_after field (numeric)
            if let Some(retry) = json
                .get("error")
                .and_then(|e| e.get("retry_after"))
                .and_then(|v| v.as_u64())
            {
                return Some(retry);
            }
        }
    }

    // B. Regex fallback patterns
    // Pattern 1: "Try again in 2m 30s"
    if let Ok(re) = Regex::new(r"(?i)try again in (\d+)m\s*(\d+)s") {
        if let Some(caps) = re.captures(body) {
            if let (Ok(m), Ok(s)) = (caps[1].parse::<u64>(), caps[2].parse::<u64>()) {
                return Some(m * 60 + s);
            }
        }
    }

    // Pattern 2: "Try again in 30s" or "backoff for 42s"
    if let Ok(re) = Regex::new(r"(?i)(?:try again in|backoff for|wait)\s*(\d+)s") {
        if let Some(caps) = re.captures(body) {
            if let Ok(s) = caps[1].parse::<u64>() {
                return Some(s);
            }
        }
    }

    // Pattern 3: "quota will reset in X seconds"
    if let Ok(re) = Regex::new(r"(?i)quota will reset in (\d+) second") {
        if let Some(caps) = re.captures(body) {
            if let Ok(s) = caps[1].parse::<u64>() {
                return Some(s);
            }
        }
    }

    // Pattern 4: OpenAI style "Retry after (\d+) seconds"
    if let Ok(re) = Regex::new(r"(?i)retry after (\d+) second") {
        if let Some(caps) = re.captures(body) {
            if let Ok(s) = caps[1].parse::<u64>() {
                return Some(s);
            }
        }
    }

    // Pattern 5: Bracket form "(wait (\d+)s)"
    if let Ok(re) = Regex::new(r"\(wait (\d+)s\)") {
        if let Some(caps) = re.captures(body) {
            if let Ok(s) = caps[1].parse::<u64>() {
                return Some(s);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_retry_time_minutes_seconds() {
        let body = "Rate limit exceeded. Try again in 2m 30s";
        let time = parse_retry_time_from_body(body);
        assert_eq!(time, Some(150));
    }

    #[test]
    fn test_parse_google_json_delay() {
        let body = r#"{
            "error": {
                "details": [
                    { 
                        "metadata": {
                            "quotaResetDelay": "42s" 
                        }
                    }
                ]
            }
        }"#;
        let time = parse_retry_time_from_body(body);
        assert_eq!(time, Some(42));
    }

    #[test]
    fn test_parse_retry_after_ignore_case() {
        let body = "Quota limit hit. Retry After 99 Seconds";
        let time = parse_retry_time_from_body(body);
        assert_eq!(time, Some(99));
    }

    #[test]
    fn test_tpm_exhausted_is_rate_limit_exceeded() {
        let body = "Resource has been exhausted (e.g. check quota). Quota limit 'Tokens per minute' exceeded.";
        let reason = parse_rate_limit_reason(body);
        assert_eq!(reason, RateLimitReason::RateLimitExceeded);
    }
}
