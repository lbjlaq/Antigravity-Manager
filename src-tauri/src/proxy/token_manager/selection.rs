// Token Selection and Scheduling Logic

use super::manager::TokenManager;
use super::models::{ProxyToken, TokenLease};
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};

impl TokenManager {
    /// P2C pool size - select from top N candidates
    const P2C_POOL_SIZE: usize = 5;

    /// Get a token with timeout protection
    pub async fn get_token(
        &self,
        quota_group: &str,
        force_rotate: bool,
        session_id: Option<&str>,
        target_model: &str,
    ) -> Result<TokenLease, String> {
        // [FIX] Timeout for deadlock detection - reduced from 120s to 5s
        const TOKEN_ACQUISITION_TIMEOUT_SECS: u64 = 5;
        let timeout_duration = std::time::Duration::from_secs(TOKEN_ACQUISITION_TIMEOUT_SECS);
        match tokio::time::timeout(
            timeout_duration,
            self.get_token_internal(quota_group, force_rotate, session_id, target_model),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(format!(
                "Token acquisition timeout ({}s) - system too busy or deadlock detected",
                TOKEN_ACQUISITION_TIMEOUT_SECS
            )),
        }
    }

    /// Internal token selection logic
    async fn get_token_internal(
        &self,
        quota_group: &str,
        force_rotate: bool,
        session_id: Option<&str>,
        target_model: &str,
    ) -> Result<TokenLease, String> {
        // [FIX] Process pending reload accounts from quota protection
        let pending_accounts = crate::proxy::server::take_pending_reload_accounts();
        for account_id in pending_accounts {
            if let Err(e) = self.reload_account(&account_id).await {
                tracing::warn!("[Quota] Failed to reload account {}: {}", account_id, e);
            }
        }

        let mut tokens_snapshot: Vec<ProxyToken> =
            self.tokens.iter().map(|e| e.value().clone()).collect();
        let total = tokens_snapshot.len();
        if total == 0 {
            return Err("Token pool is empty".to_string());
        }

        // Normalize target model
        let normalized_target =
            crate::proxy::common::model_mapping::normalize_to_standard_id(target_model)
                .unwrap_or_else(|| target_model.to_string());

        // Check quota protection config
        let quota_protection_enabled = crate::modules::config::load_app_config()
            .map(|cfg| cfg.quota_protection.enabled)
            .unwrap_or(false);

        // ===== [FIX #820] Fixed Account Mode: Prioritize preferred account =====
        let preferred_id = self.preferred_account_id.read().await.clone();
        if let Some(ref pref_id) = preferred_id {
            if let Some(preferred_token) = tokens_snapshot.iter().find(|t| &t.account_id == pref_id)
            {
                let is_rate_limited = self
                    .is_rate_limited(&preferred_token.account_id, Some(&normalized_target))
                    .await;
                let is_quota_protected = quota_protection_enabled
                    && preferred_token
                        .protected_models
                        .contains(&normalized_target);

                if !is_rate_limited && !is_quota_protected {
                    tracing::info!(
                        "🔒 [FIX #820] Using preferred account: {} (fixed mode)",
                        preferred_token.email
                    );

                    let mut token = preferred_token.clone();

                    // Refresh token if needed (5 min before expiry)
                    let now = chrono::Utc::now().timestamp();
                    if now >= token.timestamp - 300 {
                        tracing::debug!("Preferred account {} token expiring, refreshing...", token.email);
                        match crate::modules::oauth::refresh_access_token(&token.refresh_token).await {
                            Ok(token_response) => {
                                token.access_token = token_response.access_token.clone();
                                token.expires_in = token_response.expires_in;
                                token.timestamp = now + token_response.expires_in;

                                if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                                    entry.access_token = token.access_token.clone();
                                    entry.expires_in = token.expires_in;
                                    entry.timestamp = token.timestamp;
                                }
                                let _ = self.save_refreshed_token(&token.account_id, &token_response).await;
                            }
                            Err(e) => {
                                tracing::warn!("Preferred account token refresh failed: {}", e);
                            }
                        }
                    }

                    // Ensure project_id exists
                    let project_id = if let Some(pid) = &token.project_id {
                        pid.clone()
                    } else {
                        match crate::proxy::project_resolver::fetch_project_id(&token.access_token).await {
                            Ok(pid) => {
                                if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                                    entry.project_id = Some(pid.clone());
                                }
                                let _ = self.save_project_id(&token.account_id, &pid).await;
                                pid
                            }
                            Err(_) => "bamboo-precept-lgxtn".to_string(), // fallback
                        }
                    };

                    // Increment active requests
                    self.active_requests
                        .entry(token.account_id.clone())
                        .or_insert(AtomicUsize::new(0))
                        .fetch_add(1, Ordering::SeqCst);

                    return Ok(TokenLease {
                        access_token: token.access_token,
                        project_id,
                        email: token.email,
                        account_id: token.account_id.clone(),
                        active_requests: self.active_requests.clone(),
                    });
                } else {
                    if is_rate_limited {
                        tracing::warn!("🔒 [FIX #820] Preferred account {} is rate-limited, falling back to round-robin", preferred_token.email);
                    } else {
                        tracing::warn!("🔒 [FIX #820] Preferred account {} is quota-protected for {}, falling back to round-robin", preferred_token.email, target_model);
                    }
                }
            } else {
                tracing::warn!("🔒 [FIX #820] Preferred account {} not found in pool, falling back to round-robin", pref_id);
            }
        }
        // ===== [END FIX #820] =====

        // Check circuit breaker config
        let cb_enabled = self.circuit_breaker_config.read().await.enabled;

        // Filter tokens based on quota and circuit breaker
        tokens_snapshot.retain(|t| {
            // [FIX] Validation blocked check (VALIDATION_REQUIRED temporary block)
            if t.validation_blocked {
                let now = chrono::Utc::now().timestamp();
                if now < t.validation_blocked_until {
                    tracing::debug!(
                        "  ⛔ {} - SKIP: Validation blocked until {}",
                        t.email,
                        t.validation_blocked_until
                    );
                    return false;
                }
            }

            // Circuit breaker check
            if cb_enabled {
                if let Some(fail_entry) = self.circuit_breaker.get(&t.account_id) {
                    let (fail_time, reason) = fail_entry.value();
                    if fail_time.elapsed().as_secs() < 600 {
                        tracing::debug!(
                            "  ⛔ {} - SKIP: Circuit Breaker blocked ({})",
                            t.email,
                            reason
                        );
                        return false;
                    } else {
                        drop(fail_entry);
                        self.circuit_breaker.remove(&t.account_id);
                    }
                }
            }

            // Model quota check
            if let Some(&pct) = t.model_quotas.get(target_model) {
                if pct <= 0 {
                    return false;
                }
            }

            if normalized_target != target_model {
                if let Some(&pct) = t.model_quotas.get(&normalized_target) {
                    if pct <= 0 {
                        return false;
                    }
                }
            }

            // Fuzzy match for related models
            if !t.model_quotas.is_empty() {
                let is_related = |a: &str, b: &str| -> bool {
                    if a == b {
                        return true;
                    }
                    if a.len() > b.len() {
                        a.starts_with(b)
                            && a.chars()
                                .nth(b.len())
                                .map_or(false, |c| c == '-' || c == '.' || c == ':')
                    } else {
                        b.starts_with(a)
                            && b.chars()
                                .nth(a.len())
                                .map_or(false, |c| c == '-' || c == '.' || c == ':')
                    }
                };

                for (quota_model, &pct) in &t.model_quotas {
                    if pct <= 0
                        && (is_related(target_model, quota_model)
                            || is_related(normalized_target.as_str(), quota_model))
                    {
                        tracing::debug!(
                            "  ⛔ {} - SKIP: Zero quota for related model '{}' (Target: '{}')",
                            t.email,
                            quota_model,
                            target_model
                        );
                        return false;
                    }
                }
            }

            true
        });

        if tokens_snapshot.is_empty() {
            return Err(format!(
                "No accounts available with remaining quota > 0 for model '{}'",
                target_model
            ));
        }

        // Sort tokens by priority
        self.sort_tokens(&mut tokens_snapshot);

        // Log top candidates
        tracing::debug!(
            "🔄 [Token Rotation] Candidates (Top 5): {:?}",
            tokens_snapshot
                .iter()
                .take(5)
                .map(|t| {
                    let active = self
                        .active_requests
                        .get(&t.account_id)
                        .map(|c| c.load(Ordering::SeqCst))
                        .unwrap_or(0);
                    format!(
                        "{} [Active:{}, T:{:?}, Q:{:?}]",
                        t.email, active, t.subscription_tier, t.remaining_quota
                    )
                })
                .collect::<Vec<_>>()
        );

        // Apply scheduling mode filters
        let scheduling = self.sticky_config.read().await.clone();
        tracing::info!(
            "🔍 [Debug] get_token_internal | Mode: {:?} | Selected Accs: {} | Target: {}",
            scheduling.mode,
            scheduling.selected_accounts.len(),
            target_model
        );

        use crate::proxy::sticky_config::SchedulingMode;

        // [FIX] Store original tokens for potential fallback when strict_selected=false
        let all_tokens_backup = if scheduling.mode == SchedulingMode::Selected && !scheduling.strict_selected {
            Some(tokens_snapshot.clone())
        } else {
            None
        };

        if scheduling.mode == SchedulingMode::Selected {
            let selected_set: HashSet<&String> = scheduling.selected_accounts.iter().collect();

            tokens_snapshot.retain(|t| {
                if !selected_set.contains(&t.account_id) {
                    return false;
                }

                if let Some(allowed_models) = scheduling.selected_models.get(&t.account_id) {
                    if !allowed_models.is_empty() {
                        let is_allowed = allowed_models.iter().any(|m| {
                            m == target_model
                                || m == &normalized_target
                                || target_model.contains(m)
                                || m.contains(target_model)
                        });

                        if !is_allowed {
                            return false;
                        }
                    }
                }

                true
            });

            if tokens_snapshot.is_empty() {
                // [FIX] Handle strict_selected logic
                if scheduling.strict_selected {
                    // Strict mode: fail immediately, no fallback
                    return Err(format!(
                        "Selected mode (strict) is active but no valid accounts match the selection for model '{}'. No fallback allowed.",
                        target_model
                    ));
                } else if let Some(backup) = all_tokens_backup {
                    // Non-strict mode: fallback to all available accounts
                    tracing::warn!(
                        "🔄 [Selected Mode] No selected accounts available for model '{}', falling back to all {} accounts",
                        target_model,
                        backup.len()
                    );
                    tokens_snapshot = backup;
                } else {
                    return Err(format!(
                        "Selected mode is active but no valid accounts match the selection for model '{}'.",
                        target_model
                    ));
                }
            } else {
                tracing::debug!(
                    "🎯 [Selected Mode] Using subset of {} accounts for model {}{}",
                    tokens_snapshot.len(),
                    target_model,
                    if scheduling.strict_selected { " (strict)" } else { "" }
                );
            }
        }

        // [FIX] quota_protection_enabled already loaded above (line ~66), reusing value

        let total = tokens_snapshot.len();
        let last_used_account_id = if quota_group != "image_gen" {
            let last_used = self.last_used_account.lock().await;
            last_used.clone()
        } else {
            None
        };

        let mut attempted: HashSet<String> = HashSet::new();
        let mut last_error: Option<String> = None;
        let mut need_update_last_used: Option<(String, std::time::Instant)> = None;

        for attempt in 0..total {
            let rotate = force_rotate || attempt > 0;
            let mut target_token: Option<ProxyToken> = None;

            // Sticky session handling
            if !rotate
                && session_id.is_some()
                && scheduling.mode != SchedulingMode::PerformanceFirst
            {
                target_token = self
                    .try_sticky_session(
                        session_id.unwrap(),
                        &tokens_snapshot,
                        &attempted,
                        &normalized_target,
                        quota_protection_enabled,
                        &scheduling,
                    )
                    .await;
            }

            // 60s lock handling
            if target_token.is_none()
                && !rotate
                && quota_group != "image_gen"
                && scheduling.mode != SchedulingMode::PerformanceFirst
            {
                if let Some((account_id, last_time)) = &last_used_account_id {
                    if last_time.elapsed().as_secs() < 60 && !attempted.contains(account_id) {
                        if let Some(found) =
                            tokens_snapshot.iter().find(|t| &t.account_id == account_id)
                        {
                            if !self
                                .is_rate_limited(&found.account_id, Some(&normalized_target))
                                .await
                                && !(quota_protection_enabled
                                    && found.protected_models.contains(&normalized_target))
                            {
                                tracing::debug!(
                                    "60s Window: Force reusing last account: {}",
                                    found.email
                                );
                                target_token = Some(found.clone());
                            }
                        }
                    }
                }

                // Round-robin selection
                if target_token.is_none() {
                    target_token = self
                        .select_round_robin(
                            &tokens_snapshot,
                            &mut attempted,
                            &normalized_target,
                            quota_protection_enabled,
                            session_id,
                            &scheduling,
                            &mut need_update_last_used,
                        )
                        .await;
                }
            } else if target_token.is_none() {
                // Pure round-robin
                target_token = self
                    .select_round_robin(
                        &tokens_snapshot,
                        &mut attempted,
                        &normalized_target,
                        quota_protection_enabled,
                        session_id,
                        &scheduling,
                        &mut need_update_last_used,
                    )
                    .await;
            }

            let mut token = match target_token {
                Some(t) => t,
                None => {
                    // Optimistic reset strategy
                    let min_wait = tokens_snapshot
                        .iter()
                        .filter_map(|t| self.rate_limit_tracker.get_reset_seconds(&t.account_id))
                        .min();

                    if let Some(wait_sec) = min_wait {
                        if wait_sec <= 2 {
                            let wait_ms = (wait_sec as f64 * 1000.0) as u64;
                            tracing::warn!(
                                "All accounts rate-limited but shortest wait is {}s. Applying {}ms buffer...",
                                wait_sec, wait_ms
                            );

                            tokio::time::sleep(tokio::time::Duration::from_millis(wait_ms)).await;

                            let retry_token = tokens_snapshot.iter().find(|t| {
                                !attempted.contains(&t.account_id)
                                    && !self.is_rate_limited_sync(&t.account_id, None)
                            });

                            if let Some(t) = retry_token {
                                tracing::info!(
                                    "✅ Buffer delay successful! Found available account: {}",
                                    t.email
                                );
                                t.clone()
                            } else {
                                // [FIX] Use clear_expired_with_buffer instead of clear_all
                                // This only clears records expiring within 5s, preserving
                                // long-term QUOTA_EXHAUSTED locks to prevent cascade 429s
                                tracing::warn!(
                                    "Buffer delay failed. Executing safe optimistic reset (5s buffer)..."
                                );
                                let cleared = self.rate_limit_tracker.clear_expired_with_buffer(5);

                                let final_token = tokens_snapshot
                                    .iter()
                                    .find(|t| !attempted.contains(&t.account_id));

                                if let Some(t) = final_token {
                                    tracing::info!(
                                        "✅ Optimistic reset successful! Cleared {} record(s), using account: {}",
                                        cleared,
                                        t.email
                                    );
                                    t.clone()
                                } else {
                                    return Err(
                                        "All accounts failed after optimistic reset.".to_string()
                                    );
                                }
                            }
                        } else {
                            return Err(format!("All accounts limited. Wait {}s.", wait_sec));
                        }
                    } else {
                        return Err("All accounts failed or unhealthy.".to_string());
                    }
                }
            };

            // Refresh token if needed
            if let Err(e) = self.try_refresh_token(&mut token, &mut attempted, &mut last_error, quota_group, &last_used_account_id, &mut need_update_last_used).await {
                if e == "continue" {
                    continue;
                }
                return Err(e);
            }

            // Ensure project ID
            let project_id = match self.ensure_project_id(&mut token, &mut attempted, &mut last_error, quota_group, &last_used_account_id, &mut need_update_last_used).await {
                Ok(pid) => pid,
                Err(e) => {
                    if e == "continue" {
                        continue;
                    }
                    return Err(e);
                }
            };

            // Update last used if needed
            if let Some((new_account_id, new_time)) = need_update_last_used {
                if quota_group != "image_gen" {
                    let mut last_used = self.last_used_account.lock().await;
                    if new_account_id.is_empty() {
                        *last_used = None;
                    } else {
                        *last_used = Some((new_account_id, new_time));
                    }
                }
            }

            // Increment active requests
            self.active_requests
                .entry(token.account_id.clone())
                .or_insert(AtomicUsize::new(0))
                .fetch_add(1, Ordering::SeqCst);

            let active_count = self
                .active_requests
                .get(&token.account_id)
                .unwrap()
                .load(Ordering::SeqCst);
            tracing::debug!(
                "⬆️ Connection acquired: {} (active: {})",
                token.email,
                active_count
            );

            return Ok(TokenLease {
                access_token: token.access_token,
                project_id,
                email: token.email,
                account_id: token.account_id.clone(),
                active_requests: self.active_requests.clone(),
            });
        }

        Err(last_error.unwrap_or_else(|| "All accounts failed".to_string()))
    }

    /// Sort tokens by priority (tier, health, reset_time, connections, quota)
    fn sort_tokens(&self, tokens: &mut Vec<ProxyToken>) {
        // [FIX] Reset time threshold: differences < 10 minutes are considered equal priority
        const RESET_TIME_THRESHOLD_SECS: i64 = 600;

        tokens.sort_by(|a, b| {
            let get_concurrency_limit = |tier: &Option<String>| -> usize {
                match tier.as_deref() {
                    Some(t) if t.contains("ultra") => 8,
                    Some(t) if t.contains("pro") => 3,
                    Some(_) => 1,
                    None => 1,
                }
            };

            let limit_a = get_concurrency_limit(&a.subscription_tier);
            let limit_b = get_concurrency_limit(&b.subscription_tier);

            let active_a = self
                .active_requests
                .get(&a.account_id)
                .map(|c| c.load(Ordering::SeqCst))
                .unwrap_or(0);
            let active_b = self
                .active_requests
                .get(&b.account_id)
                .map(|c| c.load(Ordering::SeqCst))
                .unwrap_or(0);

            let overloaded_a = active_a >= limit_a;
            let overloaded_b = active_b >= limit_b;

            // 1. Overloaded accounts go last
            if overloaded_a != overloaded_b {
                if overloaded_a {
                    return std::cmp::Ordering::Greater;
                } else {
                    return std::cmp::Ordering::Less;
                }
            }

            // 2. Compare by subscription tier (ULTRA > PRO > FREE)
            let tier_priority = |tier: &Option<String>| match tier.as_deref() {
                Some(t) if t.contains("ultra") || t.contains("ULTRA") => 0,
                Some(t) if t.contains("pro") || t.contains("PRO") => 1,
                Some(t) if t.contains("free") || t.contains("FREE") => 2,
                _ => 3,
            };

            let tier_cmp =
                tier_priority(&a.subscription_tier).cmp(&tier_priority(&b.subscription_tier));
            if tier_cmp != std::cmp::Ordering::Equal {
                return tier_cmp;
            }

            // 3. Compare by health score (higher is better)
            let health_cmp = b
                .health_score
                .partial_cmp(&a.health_score)
                .unwrap_or(std::cmp::Ordering::Equal);
            if health_cmp != std::cmp::Ordering::Equal {
                return health_cmp;
            }

            // 4. [FIX] Compare by reset time (earlier/closer is better)
            // Differences < 10 minutes are considered equal priority to avoid frequent switching
            let reset_a = a.reset_time.unwrap_or(i64::MAX);
            let reset_b = b.reset_time.unwrap_or(i64::MAX);
            let reset_diff = (reset_a - reset_b).abs();

            if reset_diff >= RESET_TIME_THRESHOLD_SECS {
                let reset_cmp = reset_a.cmp(&reset_b);
                if reset_cmp != std::cmp::Ordering::Equal {
                    return reset_cmp;
                }
            }

            // 5. Compare by active connections (fewer is better)
            let active_cmp = active_a.cmp(&active_b);
            if active_cmp != std::cmp::Ordering::Equal {
                return active_cmp;
            }

            // 6. Compare by remaining quota (higher is better)
            let quota_a = a.remaining_quota.unwrap_or(0);
            let quota_b = b.remaining_quota.unwrap_or(0);
            quota_b.cmp(&quota_a)
        });
    }

    /// Try to use sticky session
    async fn try_sticky_session(
        &self,
        session_id: &str,
        tokens_snapshot: &[ProxyToken],
        attempted: &HashSet<String>,
        normalized_target: &str,
        quota_protection_enabled: bool,
        scheduling: &crate::proxy::sticky_config::StickySessionConfig,
    ) -> Option<ProxyToken> {
        use crate::proxy::sticky_config::SchedulingMode;

        if let Some(bound_entry) = self.session_accounts.get(session_id) {
            let (bound_id, _) = bound_entry.value();
            let bound_id = bound_id.clone();
            drop(bound_entry);

            if let Some(bound_token) = tokens_snapshot.iter().find(|t| t.account_id == bound_id) {
                let key = self
                    .email_to_account_id(&bound_token.email)
                    .unwrap_or_else(|| bound_token.account_id.clone());
                let reset_sec = self
                    .rate_limit_tracker
                    .get_remaining_wait(&key, Some(normalized_target));

                if reset_sec > 0
                    && scheduling.mode == SchedulingMode::CacheFirst
                    && reset_sec <= scheduling.max_wait_seconds
                {
                    tracing::info!(
                        "Sticky Session: Account {} limited ({}s), waiting...",
                        bound_token.email,
                        reset_sec
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(reset_sec)).await;
                }

                let reset_sec_after_wait = self
                    .rate_limit_tracker
                    .get_remaining_wait(&key, Some(normalized_target));

                if reset_sec_after_wait > 0 {
                    tracing::debug!(
                        "Sticky Session: Bound account {} is rate-limited, unbinding.",
                        bound_token.email
                    );
                    self.session_accounts.remove(session_id);
                } else if !attempted.contains(&bound_id)
                    && !(quota_protection_enabled
                        && bound_token.protected_models.contains(normalized_target))
                {
                    tracing::debug!(
                        "Sticky Session: Reusing bound account {} for session {}",
                        bound_token.email,
                        session_id
                    );
                    if let Some(mut entry) = self.session_accounts.get_mut(session_id) {
                        entry.value_mut().1 = std::time::Instant::now();
                    }
                    return Some(bound_token.clone());
                } else if quota_protection_enabled
                    && bound_token.protected_models.contains(normalized_target)
                {
                    tracing::debug!(
                        "Sticky Session: Bound account {} is quota-protected, unbinding.",
                        bound_token.email
                    );
                    self.session_accounts.remove(session_id);
                }
            } else {
                tracing::debug!("Sticky Session: Bound account not found, unbinding");
                self.session_accounts.remove(session_id);
            }
        }
        None
    }

    /// Select token using round-robin
    async fn select_round_robin(
        &self,
        tokens_snapshot: &[ProxyToken],
        attempted: &mut HashSet<String>,
        normalized_target: &str,
        quota_protection_enabled: bool,
        session_id: Option<&str>,
        scheduling: &crate::proxy::sticky_config::StickySessionConfig,
        need_update_last_used: &mut Option<(String, std::time::Instant)>,
    ) -> Option<ProxyToken> {
        use crate::proxy::sticky_config::SchedulingMode;

        let total = tokens_snapshot.len();
        if total == 0 {
            return None;
        }

        // [FIX] Safe modulo operation to prevent race condition when pool size changes
        // Use wrapping arithmetic to handle index overflow gracefully
        let raw_index = self.current_index.fetch_add(1, Ordering::SeqCst);
        let start_idx = raw_index % total;

        for offset in 0..total {
            let idx = (start_idx + offset) % total;
            let candidate = &tokens_snapshot[idx];

            if attempted.contains(&candidate.account_id) {
                continue;
            }

            if quota_protection_enabled && candidate.protected_models.contains(normalized_target) {
                continue;
            }

            if self
                .is_rate_limited(&candidate.account_id, Some(normalized_target))
                .await
            {
                continue;
            }

            *need_update_last_used = Some((candidate.account_id.clone(), std::time::Instant::now()));

            if let Some(sid) = session_id {
                if scheduling.mode != SchedulingMode::PerformanceFirst {
                    self.session_accounts.insert(
                        sid.to_string(),
                        (candidate.account_id.clone(), std::time::Instant::now()),
                    );
                    tracing::debug!(
                        "Sticky Session: Bound new account {} to session {}",
                        candidate.email,
                        sid
                    );
                }
            }

            return Some(candidate.clone());
        }

        None
    }

    /// Try to refresh token if needed
    async fn try_refresh_token(
        &self,
        token: &mut ProxyToken,
        attempted: &mut HashSet<String>,
        last_error: &mut Option<String>,
        quota_group: &str,
        last_used_account_id: &Option<(String, std::time::Instant)>,
        need_update_last_used: &mut Option<(String, std::time::Instant)>,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp();
        if now >= token.timestamp - 300 {
            tracing::debug!("账号 {} 的 token 即将过期，正在刷新...", token.email);

            match crate::modules::oauth::refresh_access_token(&token.refresh_token).await {
                Ok(token_response) => {
                    tracing::debug!("Token 刷新成功！");
                    token.access_token = token_response.access_token.clone();
                    token.expires_in = token_response.expires_in;
                    token.timestamp = now + token_response.expires_in;

                    if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                        entry.access_token = token.access_token.clone();
                        entry.expires_in = token.expires_in;
                        entry.timestamp = token.timestamp;
                    }

                    if let Err(e) = self
                        .save_refreshed_token(&token.account_id, &token_response)
                        .await
                    {
                        tracing::debug!("保存刷新后的 token 失败 ({}): {}", token.email, e);
                    }
                }
                Err(e) => {
                    tracing::error!("Token 刷新失败 ({}): {}", token.email, e);
                    if e.contains("\"invalid_grant\"") || e.contains("invalid_grant") {
                        tracing::error!(
                            "Disabling account due to invalid_grant ({})",
                            token.email
                        );
                        let _ = self
                            .disable_account(&token.account_id, &format!("invalid_grant: {}", e))
                            .await;
                        self.tokens.remove(&token.account_id);
                    }
                    *last_error = Some(format!("Token refresh failed: {}", e));
                    attempted.insert(token.account_id.clone());

                    if quota_group != "image_gen" {
                        if matches!(last_used_account_id, Some((id, _)) if id == &token.account_id)
                        {
                            *need_update_last_used =
                                Some((String::new(), std::time::Instant::now()));
                        }
                    }
                    return Err("continue".to_string());
                }
            }
        }
        Ok(())
    }

    /// Ensure token has project ID
    async fn ensure_project_id(
        &self,
        token: &mut ProxyToken,
        attempted: &mut HashSet<String>,
        last_error: &mut Option<String>,
        quota_group: &str,
        last_used_account_id: &Option<(String, std::time::Instant)>,
        need_update_last_used: &mut Option<(String, std::time::Instant)>,
    ) -> Result<String, String> {
        if let Some(pid) = &token.project_id {
            return Ok(pid.clone());
        }

        tracing::debug!("账号 {} 缺少 project_id，尝试获取...", token.email);
        match crate::proxy::project_resolver::fetch_project_id(&token.access_token).await {
            Ok(pid) => {
                if let Some(mut entry) = self.tokens.get_mut(&token.account_id) {
                    entry.project_id = Some(pid.clone());
                }
                let _ = self.save_project_id(&token.account_id, &pid).await;
                Ok(pid)
            }
            Err(e) => {
                tracing::error!("Failed to fetch project_id for {}: {}", token.email, e);
                *last_error = Some(format!("Failed to fetch project_id for {}: {}", token.email, e));
                attempted.insert(token.account_id.clone());

                if quota_group != "image_gen" {
                    if matches!(last_used_account_id, Some((id, _)) if id == &token.account_id) {
                        *need_update_last_used = Some((String::new(), std::time::Instant::now()));
                    }
                }
                Err("continue".to_string())
            }
        }
    }

    /// Check if there are available accounts for a model
    pub async fn has_available_account(&self, _quota_group: &str, target_model: &str) -> bool {
        let quota_protection_enabled = crate::modules::config::load_app_config()
            .map(|cfg| cfg.quota_protection.enabled)
            .unwrap_or(false);

        for entry in self.tokens.iter() {
            let token = entry.value();

            if self.is_rate_limited(&token.account_id, None).await {
                continue;
            }

            if quota_protection_enabled && token.protected_models.contains(target_model) {
                continue;
            }

            return true;
        }

        tracing::info!(
            "[Fallback Check] No available Google accounts for model {}",
            target_model
        );
        false
    }

    /// Power of 2 Choices (P2C) selection algorithm
    /// Randomly selects 2 from top 5 candidates, returns the one with higher quota
    /// This avoids "hot spot" issues where all requests go to the same account
    ///
    /// # Arguments
    /// * `candidates` - Pre-sorted candidate token list
    /// * `attempted` - Set of already-attempted account IDs
    /// * `normalized_target` - Normalized target model name
    /// * `quota_protection_enabled` - Whether quota protection is enabled
    #[allow(dead_code)]
    fn select_with_p2c<'a>(
        &self,
        candidates: &'a [ProxyToken],
        attempted: &HashSet<String>,
        normalized_target: &str,
        quota_protection_enabled: bool,
    ) -> Option<&'a ProxyToken> {
        use rand::Rng;

        // Filter available tokens
        let available: Vec<&ProxyToken> = candidates
            .iter()
            .filter(|t| !attempted.contains(&t.account_id))
            .filter(|t| !quota_protection_enabled || !t.protected_models.contains(normalized_target))
            .collect();

        if available.is_empty() {
            return None;
        }
        if available.len() == 1 {
            return Some(available[0]);
        }

        // P2C: randomly select 2 from top min(P2C_POOL_SIZE, len) candidates
        let pool_size = available.len().min(Self::P2C_POOL_SIZE);
        let mut rng = rand::thread_rng();

        let pick1 = rng.gen_range(0..pool_size);
        let mut pick2 = rng.gen_range(0..pool_size);
        // Ensure we pick two different candidates
        if pick2 == pick1 {
            pick2 = (pick1 + 1) % pool_size;
        }

        let c1 = available[pick1];
        let c2 = available[pick2];

        // Select the one with higher quota
        let selected = if c1.remaining_quota.unwrap_or(0) >= c2.remaining_quota.unwrap_or(0) {
            c1
        } else {
            c2
        };

        tracing::debug!(
            "🎲 [P2C] Selected {} ({}%) from [{}({}%), {}({}%)]",
            selected.email,
            selected.remaining_quota.unwrap_or(0),
            c1.email,
            c1.remaining_quota.unwrap_or(0),
            c2.email,
            c2.remaining_quota.unwrap_or(0)
        );

        Some(selected)
    }
}
