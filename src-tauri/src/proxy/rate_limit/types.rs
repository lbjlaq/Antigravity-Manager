// File: src-tauri/src/proxy/rate_limit/types.rs
//! Rate limit types and data structures.

use std::time::SystemTime;

/// Rate limit reason types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RateLimitReason {
    /// Quota exhausted (QUOTA_EXHAUSTED)
    QuotaExhausted,
    /// Rate limit exceeded (RATE_LIMIT_EXCEEDED)
    RateLimitExceeded,
    /// Model capacity exhausted (MODEL_CAPACITY_EXHAUSTED)
    ModelCapacityExhausted,
    /// Server error (5xx)
    ServerError,
    /// Unknown reason
    Unknown,
}

/// Rate limit information
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RateLimitInfo {
    /// Rate limit reset time
    pub reset_time: SystemTime,
    /// Retry interval (seconds)
    #[allow(dead_code)]
    pub retry_after_sec: u64,
    /// Detection time
    #[allow(dead_code)]
    pub detected_at: SystemTime,
    /// Rate limit reason
    #[allow(dead_code)]
    pub reason: RateLimitReason,
    /// Associated model (for model-level rate limiting)
    /// None = account-level, Some(model) = specific model
    #[allow(dead_code)]
    pub model: Option<String>,
}
