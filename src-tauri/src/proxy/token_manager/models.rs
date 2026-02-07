// Token Manager Data Models

use dashmap::DashMap;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// RAII Guard for token lease - automatically releases connection on drop
#[derive(Debug)]
pub struct TokenLease {
    pub access_token: String,
    pub project_id: String,
    pub email: String,
    pub account_id: String,
    pub(crate) active_requests: Arc<DashMap<String, AtomicUsize>>,
}

impl Drop for TokenLease {
    fn drop(&mut self) {
        if let Some(counter) = self.active_requests.get(&self.account_id) {
            counter.fetch_sub(1, Ordering::SeqCst);
            tracing::debug!(
                "⬇️ Connection released: {} (active: {})",
                self.email,
                counter.load(Ordering::SeqCst)
            );
        }
    }
}

/// Represents a proxy-enabled Google account token
#[derive(Debug, Clone)]
pub struct ProxyToken {
    pub account_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub timestamp: i64,
    pub email: String,
    pub account_path: PathBuf,
    pub project_id: Option<String>,
    pub subscription_tier: Option<String>, // "FREE" | "PRO" | "ULTRA"
    pub remaining_quota: Option<i32>,
    pub protected_models: HashSet<String>,
    pub health_score: f32,
    pub model_quotas: HashMap<String, i32>,
    pub verification_needed: bool,
    pub verification_url: Option<String>,
    pub reset_time: Option<i64>,        // [FIX] Quota reset timestamp for priority sorting
    pub validation_blocked: bool,       // [FIX] Temporary block for VALIDATION_REQUIRED
    pub validation_blocked_until: i64,  // [FIX] Timestamp until which account is blocked
    pub is_forbidden: bool,
}
