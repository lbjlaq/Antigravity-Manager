// Token Manager Core Structure

use dashmap::DashMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use super::models::ProxyToken;
use crate::proxy::rate_limit::RateLimitTracker;
use crate::proxy::sticky_config::StickySessionConfig;

/// Central token manager for Google account pool
pub struct TokenManager {
    pub(crate) tokens: Arc<DashMap<String, ProxyToken>>,
    pub(crate) current_index: Arc<AtomicUsize>,
    pub(crate) last_used_account: Arc<tokio::sync::Mutex<Option<(String, std::time::Instant)>>>,
    pub(crate) data_dir: PathBuf,
    pub(crate) rate_limit_tracker: Arc<RateLimitTracker>,
    pub(crate) sticky_config: Arc<tokio::sync::RwLock<StickySessionConfig>>,
    pub(crate) session_accounts: Arc<DashMap<String, (String, std::time::Instant)>>,
    pub(crate) health_scores: Arc<DashMap<String, f32>>,
    pub(crate) active_requests: Arc<DashMap<String, AtomicUsize>>,
    pub(crate) circuit_breaker_config: Arc<tokio::sync::RwLock<crate::models::CircuitBreakerConfig>>,
    pub circuit_breaker: DashMap<String, (std::time::Instant, String)>,
}

impl TokenManager {
    /// Create a new TokenManager
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            tokens: Arc::new(DashMap::new()),
            current_index: Arc::new(AtomicUsize::new(0)),
            last_used_account: Arc::new(tokio::sync::Mutex::new(None)),
            data_dir,
            rate_limit_tracker: Arc::new(RateLimitTracker::new()),
            sticky_config: Arc::new(tokio::sync::RwLock::new(StickySessionConfig::default())),
            session_accounts: Arc::new(DashMap::new()),
            health_scores: Arc::new(DashMap::new()),
            active_requests: Arc::new(DashMap::new()),
            circuit_breaker_config: Arc::new(tokio::sync::RwLock::new(
                crate::models::CircuitBreakerConfig::default(),
            )),
            circuit_breaker: DashMap::new(),
        }
    }

    /// Get the number of loaded tokens
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Check if token pool is empty
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// Start auto-cleanup background task
    pub fn start_auto_cleanup(&self) {
        let tracker = self.rate_limit_tracker.clone();
        let session_map = self.session_accounts.clone();
        let circuit_breaker_clone = self.circuit_breaker.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
            let mut session_cleanup_interval = 0;

            loop {
                interval.tick().await;
                let cleaned = tracker.cleanup_expired();
                if cleaned > 0 {
                    tracing::info!(
                        "🧹 Auto-cleanup: Removed {} expired rate limit record(s)",
                        cleaned
                    );
                }

                // Clean expired circuit breaker records
                let now = std::time::Instant::now();
                let mut cb_cleaned = 0;
                circuit_breaker_clone.retain(|_, (fail_time, _)| {
                    if now.duration_since(*fail_time).as_secs() > 600 {
                        cb_cleaned += 1;
                        false
                    } else {
                        true
                    }
                });
                if cb_cleaned > 0 {
                    tracing::info!(
                        "🔓 Circuit Breaker: Unblocked {} recovered accounts",
                        cb_cleaned
                    );
                }

                // Session cleanup every 10 mins
                session_cleanup_interval += 1;
                if session_cleanup_interval >= 40 {
                    session_cleanup_interval = 0;
                    let now = std::time::Instant::now();
                    let expiry = std::time::Duration::from_secs(24 * 3600);
                    let mut removed_sessions = 0;

                    session_map.retain(|_, (_, ts)| {
                        if now.duration_since(*ts) > expiry {
                            removed_sessions += 1;
                            false
                        } else {
                            true
                        }
                    });

                    if removed_sessions > 0 {
                        tracing::info!(
                            "🧹 Session Cleanup: Removed {} expired sessions",
                            removed_sessions
                        );
                    }
                }
            }
        });
        tracing::info!("✅ Rate limit & Session auto-cleanup task started");
    }

    /// Update circuit breaker configuration at runtime
    pub async fn update_circuit_breaker_config(&self, config: crate::models::CircuitBreakerConfig) {
        let mut w = self.circuit_breaker_config.write().await;
        *w = config;
        tracing::info!("🛡️ Circuit Breaker config updated: enabled={}", w.enabled);
    }

    /// Get circuit breaker configuration
    pub async fn get_circuit_breaker_config(&self) -> crate::models::CircuitBreakerConfig {
        self.circuit_breaker_config.read().await.clone()
    }

    /// Report account failure for circuit breaker
    pub fn report_account_failure(&self, account_id: &str, status_code: u16, error_msg: &str) {
        let should_block = matches!(status_code, 402 | 429 | 401);

        if should_block {
            let now = std::time::Instant::now();
            self.circuit_breaker.insert(
                account_id.to_string(),
                (now, format!("Error {}: {}", status_code, error_msg)),
            );
            tracing::warn!(
                "🚫 [Circuit Breaker] Blocking account {} due to error {}: {}",
                account_id,
                status_code,
                error_msg
            );
        }
    }

    /// Report account needing validation (Gemini 403 VALIDATION_REQUIRED)
    pub fn report_account_validation_required(&self, account_id: &str, verification_url: &str) {
        if let Some(mut token) = self.tokens.get_mut(account_id) {
            token.verification_needed = true;
            token.verification_url = Some(verification_url.to_string());
            tracing::warn!("⚠️ Account {} marked as needing verification", token.email);

            let path = token.account_path.clone();
            drop(token);

            let url = verification_url.to_string();
            let aid = account_id.to_string();

            tokio::task::spawn_blocking(move || {
                match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&content) {
                            json["proxy_disabled"] = serde_json::Value::Bool(true);
                            json["proxy_disabled_reason"] =
                                serde_json::Value::String("verification_required".to_string());
                            json["verification_needed"] = serde_json::Value::Bool(true);
                            json["verification_url"] = serde_json::Value::String(url);

                            if let Ok(new_content) = serde_json::to_string_pretty(&json) {
                                let _ = std::fs::write(&path, new_content);
                                tracing::info!(
                                    "💾 Account {} updated on disk (Verification Required)",
                                    aid
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to update account file for verification: {}", e)
                    }
                }
            });
        }
    }

    /// Record successful request - increase health score
    pub fn record_success(&self, account_id: &str) {
        self.health_scores
            .entry(account_id.to_string())
            .and_modify(|s| *s = (*s + 0.05).min(1.0))
            .or_insert(1.0);
        tracing::debug!("📈 Health score increased for account {}", account_id);
    }

    /// Record failed request - decrease health score
    pub fn record_failure(&self, account_id: &str) {
        self.health_scores
            .entry(account_id.to_string())
            .and_modify(|s| *s = (*s - 0.2).max(0.0))
            .or_insert(0.8);
        tracing::warn!("📉 Health score decreased for account {}", account_id);
    }

    /// Report 429 penalty - heavily decrease health score
    pub fn report_429_penalty(&self, account_id: &str) {
        if let Some(mut score) = self.health_scores.get_mut(account_id) {
            let old_score = *score;
            *score = (*score * 0.5).max(0.01);
            tracing::warn!(
                "⚠️ Account {} hit 429! Health penalty: {:.2} -> {:.2}",
                account_id,
                old_score,
                *score
            );
        }
    }

    /// Get account ID by email
    pub fn get_account_id_by_email(&self, email: &str) -> Option<String> {
        for entry in self.tokens.iter() {
            if entry.email == email {
                return Some(entry.account_id.clone());
            }
        }
        None
    }

    /// Convert email to account_id (internal helper)
    pub(crate) fn email_to_account_id(&self, email: &str) -> Option<String> {
        self.tokens
            .iter()
            .find(|entry| entry.value().email == email)
            .map(|entry| entry.value().account_id.clone())
    }

    /// Get effective account count (considering scheduling mode)
    pub async fn effective_len(&self) -> usize {
        let config = self.sticky_config.read().await;
        if matches!(
            config.mode,
            crate::proxy::sticky_config::SchedulingMode::Selected
        ) {
            config.selected_accounts.len()
        } else {
            self.tokens.len()
        }
    }
}

/// Truncate long reason strings
pub(crate) fn truncate_reason(reason: &str, max_len: usize) -> String {
    if reason.len() <= max_len {
        reason.to_string()
    } else {
        format!("{}...", &reason[..max_len - 3])
    }
}
