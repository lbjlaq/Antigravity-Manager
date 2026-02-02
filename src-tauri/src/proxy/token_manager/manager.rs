// Token Manager Core Structure

use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

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
    /// [FIX #820] Preferred account ID for fixed account mode
    pub(crate) preferred_account_id: Arc<tokio::sync::RwLock<Option<String>>>,
    /// [NEW] Cancellation token for graceful shutdown
    cancel_token: CancellationToken,
    /// [NEW] Handle for auto-cleanup background task
    auto_cleanup_handle: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
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
            preferred_account_id: Arc::new(tokio::sync::RwLock::new(None)),
            cancel_token: CancellationToken::new(),
            auto_cleanup_handle: Arc::new(tokio::sync::Mutex::new(None)),
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

    /// Start auto-cleanup background task with cancellation support
    pub async fn start_auto_cleanup(&self) {
        let tracker = self.rate_limit_tracker.clone();
        let session_map = self.session_accounts.clone();
        let circuit_breaker_clone = self.circuit_breaker.clone();
        let cancel = self.cancel_token.child_token();

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
            let mut session_cleanup_interval = 0;

            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        tracing::info!("Auto-cleanup task received cancel signal");
                        break;
                    }
                    _ = interval.tick() => {
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
                }
            }
        });

        // Abort old task if exists (prevent task leak), then store new handle
        let mut guard = self.auto_cleanup_handle.lock().await;
        if let Some(old) = guard.take() {
            old.abort();
            tracing::warn!("Aborted previous auto-cleanup task");
        }
        *guard = Some(handle);

        tracing::info!("✅ Rate limit & Session auto-cleanup task started");
    }

    /// Graceful shutdown with timeout
    ///
    /// # Arguments
    /// * `timeout` - Maximum time to wait for tasks to complete
    pub async fn graceful_shutdown(&self, timeout: std::time::Duration) {
        tracing::info!("Initiating graceful shutdown of background tasks...");

        // Send cancel signal to all background tasks
        self.cancel_token.cancel();

        // Wait for tasks to complete with timeout
        match tokio::time::timeout(timeout, self.abort_background_tasks()).await {
            Ok(_) => tracing::info!("All background tasks cleaned up gracefully"),
            Err(_) => tracing::warn!(
                "Graceful cleanup timed out after {:?}, tasks were force-aborted",
                timeout
            ),
        }
    }

    /// Abort and wait for all background tasks to complete
    pub async fn abort_background_tasks(&self) {
        Self::abort_task(&self.auto_cleanup_handle, "Auto-cleanup task").await;
    }

    /// Abort a single background task and log the result
    ///
    /// # Arguments
    /// * `handle` - Mutex reference to the task handle
    /// * `task_name` - Task name for logging
    async fn abort_task(
        handle: &tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
        task_name: &str,
    ) {
        let Some(handle) = handle.lock().await.take() else {
            return;
        };

        handle.abort();
        match handle.await {
            Ok(()) => tracing::debug!("{} completed", task_name),
            Err(e) if e.is_cancelled() => tracing::info!("{} aborted", task_name),
            Err(e) => tracing::warn!("{} error: {}", task_name, e),
        }
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
        // [FIX] Check if account exists in index before writing
        let exists = match crate::modules::account::storage::load_account_index() {
            Ok(index) => index.accounts.iter().any(|s| s.id == account_id),
            Err(_) => false,
        };
        
        if !exists {
            tracing::warn!("report_account_validation_required: Account {} not in index, skipping", account_id);
            self.tokens.remove(account_id);
            return;
        }

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

    // =========================================================================
    // [FIX #820] Preferred Account Management
    // =========================================================================

    /// Set preferred account ID (fixed account mode)
    pub async fn set_preferred_account(&self, account_id: Option<String>) {
        let mut preferred = self.preferred_account_id.write().await;
        if let Some(ref id) = account_id {
            tracing::info!("[FIX #820] Preferred account set to: {}", id);
        } else {
            tracing::info!("[FIX #820] Preferred account cleared");
        }
        *preferred = account_id;
    }

    /// Get current preferred account ID
    pub async fn get_preferred_account(&self) -> Option<String> {
        self.preferred_account_id.read().await.clone()
    }

    // =========================================================================
    // [FIX] Account Removal - Prevent resurrection after delete
    // =========================================================================

    /// Remove account from TokenManager completely
    /// 
    /// This must be called BEFORE deleting the account file to prevent
    /// race conditions where persist_token() could recreate the account.
    pub fn remove_account(&self, account_id: &str) {
        // Remove from token pool
        if self.tokens.remove(account_id).is_some() {
            tracing::info!("🗑️ Removed account {} from token pool", account_id);
        }

        // Remove health score
        self.health_scores.remove(account_id);

        // Remove from circuit breaker
        self.circuit_breaker.remove(account_id);

        // Remove active request counter
        self.active_requests.remove(account_id);

        // Clear any session bindings to this account
        self.session_accounts.retain(|_, (aid, _)| aid != account_id);
    }

    /// Remove multiple accounts from TokenManager
    pub fn remove_accounts(&self, account_ids: &[String]) {
        for account_id in account_ids {
            self.remove_account(account_id);
        }
        tracing::info!("🗑️ Batch removed {} accounts from token pool", account_ids.len());
    }

    /// Check if account exists in token pool
    pub fn has_account(&self, account_id: &str) -> bool {
        self.tokens.contains_key(account_id)
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
