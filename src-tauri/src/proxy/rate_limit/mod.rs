// File: src-tauri/src/proxy/rate_limit/mod.rs
//! Rate limit tracking module.
//!
//! Tracks rate limits for accounts and models, with support for:
//! - Account-level and model-level rate limiting
//! - Exponential backoff with configurable steps
//! - Automatic expiry of failure counts
//! - Optimistic reset for near-expired records

mod types;
mod parsing;

pub use types::{RateLimitReason, RateLimitInfo};
pub use parsing::{parse_rate_limit_reason, parse_retry_time_from_body, parse_duration_string};

use dashmap::DashMap;
use std::time::{Duration, SystemTime};

/// Failure count expiry time: 1 hour (reset count if no failures within this period)
const FAILURE_COUNT_EXPIRY_SECONDS: u64 = 3600;

/// Rate limit tracker
pub struct RateLimitTracker {
    limits: DashMap<String, RateLimitInfo>,
    /// Consecutive failure counts (for intelligent exponential backoff), with timestamp for auto-expiry
    failure_counts: DashMap<String, (u32, SystemTime)>,
}

impl RateLimitTracker {
    pub fn new() -> Self {
        Self {
            limits: DashMap::new(),
            failure_counts: DashMap::new(),
        }
    }

    /// Generate rate limit key
    /// - Account-level: "account_id"
    /// - Model-level: "account_id:model_id"
    fn get_limit_key(&self, account_id: &str, model: Option<&str>) -> String {
        match model {
            Some(m) if !m.is_empty() => format!("{}:{}", account_id, m),
            _ => account_id.to_string(),
        }
    }

    /// Get remaining wait time for account (seconds)
    /// Supports checking both account-level and model-level locks
    pub fn get_remaining_wait(&self, account_id: &str, model: Option<&str>) -> u64 {
        let now = SystemTime::now();

        // 1. Check global account lock
        if let Some(info) = self.limits.get(account_id) {
            if info.reset_time > now {
                return info
                    .reset_time
                    .duration_since(now)
                    .unwrap_or(Duration::from_secs(0))
                    .as_secs();
            }
        }

        // 2. If model specified, check model-level lock
        if let Some(m) = model {
            let key = self.get_limit_key(account_id, Some(m));
            if let Some(info) = self.limits.get(&key) {
                if info.reset_time > now {
                    return info
                        .reset_time
                        .duration_since(now)
                        .unwrap_or(Duration::from_secs(0))
                        .as_secs();
                }
            }
        }

        0
    }

    /// Mark account request as successful, reset consecutive failure count
    ///
    /// Call this method after account successfully completes a request,
    /// resets failure count to zero and clears related rate limit locks.
    pub fn mark_success(&self, account_id: &str, model: Option<&str>) {
        if self.failure_counts.remove(account_id).is_some() {
            tracing::debug!("Account {} request succeeded, failure count reset", account_id);
        }

        // 1. Clear account-level global rate limit
        self.limits.remove(account_id);

        // 2. If model specified, also clear that model's specific lock
        if let Some(m) = model {
            let key = self.get_limit_key(account_id, Some(m));
            if self.limits.remove(&key).is_some() {
                tracing::debug!("Account {} model {} rate limit lock cleared", account_id, m);
            }
        }
    }

    /// Precisely lock account until specified time
    ///
    /// Uses reset_time from account quota for precise locking,
    /// more accurate than exponential backoff.
    ///
    /// # Arguments
    /// - `model`: Optional model name for model-level rate limiting. None = account-level
    pub fn set_lockout_until(
        &self,
        account_id: &str,
        reset_time: SystemTime,
        reason: RateLimitReason,
        model: Option<String>,
    ) {
        let now = SystemTime::now();
        let retry_sec = reset_time
            .duration_since(now)
            .map(|d| d.as_secs())
            .unwrap_or(60); // Use default 60 seconds if time already passed

        let info = RateLimitInfo {
            reset_time,
            retry_after_sec: retry_sec,
            detected_at: now,
            reason,
            model: model.clone(),
        };

        let key = self.get_limit_key(account_id, model.as_deref());
        self.limits.insert(key, info);

        if let Some(m) = &model {
            tracing::info!(
                "Account {} model {} precisely locked until quota refresh, {} seconds remaining",
                account_id,
                m,
                retry_sec
            );
        } else {
            tracing::info!(
                "Account {} precisely locked until quota refresh, {} seconds remaining",
                account_id,
                retry_sec
            );
        }
    }

    /// Precisely lock account using ISO 8601 time string
    ///
    /// Parses time strings like "2026-01-08T17:00:00Z"
    ///
    /// # Arguments
    /// - `model`: Optional model name for model-level rate limiting
    pub fn set_lockout_until_iso(
        &self,
        account_id: &str,
        reset_time_str: &str,
        reason: RateLimitReason,
        model: Option<String>,
    ) -> bool {
        match chrono::DateTime::parse_from_rfc3339(reset_time_str) {
            Ok(dt) => {
                let reset_time =
                    SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(dt.timestamp() as u64);
                self.set_lockout_until(account_id, reset_time, reason, model);
                true
            }
            Err(e) => {
                tracing::warn!(
                    "Cannot parse quota refresh time '{}': {}, using default backoff strategy",
                    reset_time_str,
                    e
                );
                false
            }
        }
    }

    /// Parse rate limit info from error response
    ///
    /// # Arguments
    /// * `account_id` - Account ID
    /// * `status` - HTTP status code
    /// * `retry_after_header` - Retry-After header value
    /// * `body` - Error response body
    /// * `model` - Optional model name
    /// * `backoff_steps` - Backoff configuration steps
    pub fn parse_from_error(
        &self,
        account_id: &str,
        status: u16,
        retry_after_header: Option<&str>,
        body: &str,
        model: Option<String>,
        backoff_steps: &[u64],
    ) -> Option<RateLimitInfo> {
        // Support 429 (rate limit) and 500/503/529 (backend soft avoidance)
        if status != 429 && status != 500 && status != 503 && status != 529 {
            return None;
        }

        // 1. Parse rate limit reason type
        let reason = if status == 429 {
            tracing::warn!("Google 429 Error Body: {}", body);
            parse_rate_limit_reason(body)
        } else {
            RateLimitReason::ServerError
        };

        let mut retry_after_sec = None;

        // 2. Extract from Retry-After header
        if let Some(retry_after) = retry_after_header {
            if let Ok(seconds) = retry_after.parse::<u64>() {
                retry_after_sec = Some(seconds);
            }
        }

        // 3. Extract from error message (prefer JSON, then regex)
        if retry_after_sec.is_none() {
            retry_after_sec = parse_retry_time_from_body(body);
        }

        // 4. Handle defaults and soft avoidance logic (different defaults by limit type)
        let retry_sec = match retry_after_sec {
            Some(s) => {
                // Set safety buffer: minimum 2 seconds to prevent very high frequency invalid retries
                if s < 2 { 2 } else { s }
            }
            None => {
                // Get consecutive failure count for exponential backoff (with auto-expiry logic)
                // ServerError (5xx) doesn't accumulate failure_count to avoid polluting 429 backoff ladder
                let failure_count = if reason != RateLimitReason::ServerError {
                    let now = SystemTime::now();
                    let mut entry = self
                        .failure_counts
                        .entry(account_id.to_string())
                        .or_insert((0, now));

                    let elapsed = now
                        .duration_since(entry.1)
                        .unwrap_or(Duration::from_secs(0))
                        .as_secs();
                    if elapsed > FAILURE_COUNT_EXPIRY_SECONDS {
                        tracing::debug!(
                            "Account {} failure count expired ({}s), resetting to 0",
                            account_id,
                            elapsed
                        );
                        *entry = (0, now);
                    }
                    entry.0 += 1;
                    entry.1 = now;
                    entry.0
                } else {
                    // ServerError (5xx) uses fixed value 1, doesn't accumulate
                    1
                };

                match reason {
                    RateLimitReason::QuotaExhausted => {
                        // Calculate based on failure_count and configured backoff_steps
                        let index = (failure_count as usize).saturating_sub(1);
                        let lockout = if index < backoff_steps.len() {
                            backoff_steps[index]
                        } else {
                            *backoff_steps.last().unwrap_or(&7200)
                        };

                        tracing::warn!(
                            "Detected quota exhausted (QUOTA_EXHAUSTED), consecutive failure #{}, locking for {} seconds per config",
                            failure_count,
                            lockout
                        );
                        lockout
                    }
                    RateLimitReason::RateLimitExceeded => {
                        // Rate limit (TPM/RPM)
                        tracing::debug!("Detected rate limit (RATE_LIMIT_EXCEEDED), using default 5s");
                        5
                    }
                    RateLimitReason::ModelCapacityExhausted => {
                        // Model capacity exhausted
                        let lockout = match failure_count {
                            1 => 5,
                            2 => 10,
                            _ => 15,
                        };
                        tracing::warn!(
                            "Detected model capacity exhausted (MODEL_CAPACITY_EXHAUSTED), failure #{}, retrying in {}s",
                            failure_count,
                            lockout
                        );
                        lockout
                    }
                    RateLimitReason::ServerError => {
                        // 5xx error
                        tracing::warn!("Detected 5xx error ({}), executing 8s soft avoidance...", status);
                        8
                    }
                    RateLimitReason::Unknown => {
                        // Unknown reason
                        tracing::debug!("Cannot parse 429 rate limit reason, using default 60s");
                        60
                    }
                }
            }
        };

        let info = RateLimitInfo {
            reset_time: SystemTime::now() + Duration::from_secs(retry_sec),
            retry_after_sec: retry_sec,
            detected_at: SystemTime::now(),
            reason,
            model: model.clone(),
        };

        // Use composite key for storage (if Quota and has Model)
        // Only QuotaExhausted is suitable for model isolation
        let use_model_key = matches!(reason, RateLimitReason::QuotaExhausted) && model.is_some();
        let key = if use_model_key {
            self.get_limit_key(account_id, model.as_deref())
        } else {
            account_id.to_string()
        };

        self.limits.insert(key, info.clone());

        tracing::warn!(
            "Account {} [{}] rate limit type: {:?}, reset delay: {}s",
            account_id,
            status,
            reason,
            retry_sec
        );

        Some(info)
    }

    /// Get rate limit info for account
    pub fn get(&self, account_id: &str) -> Option<RateLimitInfo> {
        self.limits.get(account_id).map(|r| r.clone())
    }

    /// Check if account is still rate limited (supports model-level)
    pub fn is_rate_limited(&self, account_id: &str, model: Option<&str>) -> bool {
        self.get_remaining_wait(account_id, model) > 0
    }

    /// Get seconds until rate limit reset
    pub fn get_reset_seconds(&self, account_id: &str) -> Option<u64> {
        if let Some(info) = self.get(account_id) {
            info.reset_time
                .duration_since(SystemTime::now())
                .ok()
                .map(|d| d.as_secs())
        } else {
            None
        }
    }

    /// Clear expired rate limit records
    #[allow(dead_code)]
    pub fn cleanup_expired(&self) -> usize {
        let now = SystemTime::now();
        let mut count = 0;

        self.limits.retain(|_k, v| {
            if v.reset_time <= now {
                count += 1;
                false
            } else {
                true
            }
        });

        if count > 0 {
            tracing::debug!("Cleared {} expired rate limit records", count);
        }

        count
    }

    /// Clear rate limit record for specified account
    pub fn clear(&self, account_id: &str) -> bool {
        self.limits.remove(account_id).is_some()
    }

    /// Clear only expired or nearly-expired rate limit records (optimistic reset)
    ///
    /// Safer than clear_all() - only clears records that have expired or
    /// will expire within the buffer_seconds threshold.
    ///
    /// # Arguments
    /// * `buffer_seconds` - Also clear records expiring within this many seconds
    ///
    /// # Returns
    /// Number of records cleared
    pub fn clear_expired_with_buffer(&self, buffer_seconds: u64) -> usize {
        let now = SystemTime::now();
        let buffer = Duration::from_secs(buffer_seconds);
        let threshold = now + buffer;
        let mut count = 0;

        self.limits.retain(|key, info| {
            if info.reset_time <= threshold {
                tracing::debug!(
                    "🧹 Optimistic reset: Clearing expired/near-expired record for {}",
                    key
                );
                count += 1;
                false
            } else {
                true
            }
        });

        if count > 0 {
            tracing::info!(
                "🔄 Optimistic reset: Cleared {} expired/near-expired rate limit record(s) (buffer: {}s)",
                count,
                buffer_seconds
            );
        }

        count
    }

    /// Clear all rate limit records (optimistic reset strategy)
    ///
    /// Used for optimistic reset mechanism, when all accounts are rate limited
    /// but wait time is very short, clear all records to resolve timing race conditions.
    ///
    /// WARNING: This clears ALL records including long-term QUOTA_EXHAUSTED locks.
    /// Prefer clear_expired_with_buffer() for safer optimistic reset.
    #[allow(dead_code)]
    pub fn clear_all(&self) {
        let count = self.limits.len();
        self.limits.clear();
        tracing::warn!(
            "🔄 Optimistic reset: Cleared all {} rate limit record(s)",
            count
        );
    }
}

impl Default for RateLimitTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_remaining_wait() {
        let tracker = RateLimitTracker::new();
        tracker.parse_from_error("acc1", 429, Some("30"), "", None, &[]);
        let wait = tracker.get_remaining_wait("acc1", None);
        assert!(wait > 25 && wait <= 30);
    }

    #[test]
    fn test_safety_buffer() {
        let tracker = RateLimitTracker::new();
        // If API returns 1s, we force to 2s
        tracker.parse_from_error("acc1", 429, Some("1"), "", None, &[]);
        let wait = tracker.get_remaining_wait("acc1", None);
        // Due to time passing, it might be 1 or 2
        assert!(wait >= 1 && wait <= 2);
    }
}
