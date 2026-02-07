// Token Selection Module
// Handles token acquisition, scheduling, and rotation logic

mod scoring;
mod sticky;
mod round_robin;
mod token_ops;
mod p2c;

use super::manager::TokenManager;
use super::models::{ProxyToken, TokenLease};
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};

impl TokenManager {
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
        if let Some(token_lease) = self.try_preferred_account(&tokens_snapshot, &normalized_target, quota_protection_enabled).await {
            return Ok(token_lease);
        }
        // ===== [END FIX #820] =====

        // Check circuit breaker config
        let cb_enabled = self.circuit_breaker_config.read().await.enabled;

        // Filter tokens based on quota and circuit breaker
        self.filter_tokens(&mut tokens_snapshot, target_model, &normalized_target, cb_enabled);

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

        // Apply selected mode filtering
        self.apply_selected_mode_filter(&mut tokens_snapshot, target_model, &normalized_target, &scheduling)?;

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
                && scheduling.mode != crate::proxy::sticky_config::SchedulingMode::PerformanceFirst
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
                && scheduling.mode != crate::proxy::sticky_config::SchedulingMode::PerformanceFirst
            {
                target_token = self.try_60s_lock(
                    &tokens_snapshot,
                    &attempted,
                    &normalized_target,
                    quota_protection_enabled,
                    &last_used_account_id,
                ).await;

                // Round-robin or P2C selection based on scheduling mode
                if target_token.is_none() {
                    target_token = self.select_by_mode(
                        &tokens_snapshot,
                        &mut attempted,
                        &normalized_target,
                        quota_protection_enabled,
                        session_id,
                        &scheduling,
                        &mut need_update_last_used,
                    ).await;
                }
            } else if target_token.is_none() {
                // Pure round-robin or P2C
                target_token = self.select_by_mode(
                    &tokens_snapshot,
                    &mut attempted,
                    &normalized_target,
                    quota_protection_enabled,
                    session_id,
                    &scheduling,
                    &mut need_update_last_used,
                ).await;
            }

            let mut token = match target_token {
                Some(t) => t,
                None => {
                    match self.try_optimistic_reset(&tokens_snapshot, &attempted).await {
                        Ok(t) => t,
                        Err(e) => return Err(e),
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

    /// Try to use preferred account (Fixed Account Mode)
    async fn try_preferred_account(
        &self,
        tokens_snapshot: &[ProxyToken],
        normalized_target: &str,
        quota_protection_enabled: bool,
    ) -> Option<TokenLease> {
        let preferred_id = self.preferred_account_id.read().await.clone();
        if let Some(ref pref_id) = preferred_id {
            if let Some(preferred_token) = tokens_snapshot.iter().find(|t| &t.account_id == pref_id)
            {
                let is_rate_limited = self
                    .is_rate_limited(&preferred_token.account_id, Some(normalized_target))
                    .await;
                let is_quota_protected = quota_protection_enabled
                    && preferred_token
                        .protected_models
                        .contains(normalized_target);

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
                        // [FIX #1583] Pass account_id for proper context
                        match crate::modules::oauth::refresh_access_token(&token.refresh_token, Some(&token.account_id)).await {
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

                    return Some(TokenLease {
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
                        tracing::warn!("🔒 [FIX #820] Preferred account {} is quota-protected for {}, falling back to round-robin", preferred_token.email, normalized_target);
                    }
                }
            } else {
                tracing::warn!("🔒 [FIX #820] Preferred account {} not found in pool, falling back to round-robin", pref_id);
            }
        }
        None
    }

    fn filter_tokens(
        &self,
        tokens_snapshot: &mut Vec<ProxyToken>,
        target_model: &str,
        normalized_target: &str,
        cb_enabled: bool,
    ) {
        let initial_count = tokens_snapshot.len();
        
        // [DIAG] Log initial state before filtering
        tracing::info!(
            "🔍 [Filter START] Model '{}' (normalized: '{}') | {} accounts in pool | CB enabled: {}",
            target_model,
            normalized_target,
            initial_count,
            cb_enabled
        );
        
        // [DIAG] Log details of each account (CHANGED TO INFO FOR DEBUGGING)
        for t in tokens_snapshot.iter() {
            tracing::info!(
                "   📋 {} | v_needed:{} | v_blocked:{} | quotas: {:?} | CB: {}",
                t.email,
                t.verification_needed,
                t.validation_blocked,
                t.model_quotas,
                self.circuit_breaker.contains_key(&t.account_id)
            );
        }

        tokens_snapshot.retain(|t| {
            // [NEW] Verification required check (permanent block until manual verification)
            if t.verification_needed {
                tracing::info!(
                    "  ⛔ {} - SKIP: Verification required (permanent)",
                    t.email
                );
                return false;
            }

            // [FIX] Validation blocked check (VALIDATION_REQUIRED temporary block)
            if t.validation_blocked {
                let now = chrono::Utc::now().timestamp();
                if now < t.validation_blocked_until {
                    tracing::info!(
                        "  ⛔ {} - SKIP: Validation blocked until {}",
                        t.email,
                        t.validation_blocked_until
                    );
                    return false;
                }
            }

            // NOTE: remaining_quota check removed - it was too aggressive
            // The model_quotas check below handles per-model quota correctly
            // remaining_quota is max percentage across ALL models, which can be 0
            // even if the target model has available quota

            // NOTE: Rate limit check is done later in select_round_robin() and try_60s_lock()
            // Cannot use is_rate_limited_sync() here as it causes blocking_read() deadlock in async context

            // Circuit breaker check
            if cb_enabled {
                if let Some(fail_entry) = self.circuit_breaker.get(&t.account_id) {
                    let (fail_time, reason) = fail_entry.value();
                    if fail_time.elapsed().as_secs() < 600 {
                        tracing::info!(
                            "  ⛔ {} - SKIP: Circuit Breaker blocked ({}) [{}s remaining]",
                            t.email,
                            reason,
                            600 - fail_time.elapsed().as_secs()
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
                    tracing::info!(
                        "  ⛔ {} - SKIP: Zero quota for target model '{}' (pct={})",
                        t.email, target_model, pct
                    );
                    return false;
                }
            }

            if normalized_target != target_model {
                if let Some(&pct) = t.model_quotas.get(normalized_target) {
                    if pct <= 0 {
                        tracing::info!(
                            "  ⛔ {} - SKIP: Zero quota for normalized model '{}' (pct={})",
                            t.email, normalized_target, pct
                        );
                        return false;
                    }
                }
            }

            // Fuzzy match for related models
            // [FIX] Only block if quota_model is MORE SPECIFIC (longer) than target
            // This prevents "claude-opus-4: 0%" from blocking "claude-opus-4-5-thinking"
            // But allows "claude-opus-4-5-thinking: 0%" to block "claude-opus-4-5-thinking"
            if !t.model_quotas.is_empty() {
                // Check if target is a prefix of quota_model (quota_model is more specific)
                let is_more_specific_variant = |quota_model: &str, target: &str| -> bool {
                    quota_model.len() > target.len()
                        && quota_model.starts_with(target)
                        && quota_model.chars()
                            .nth(target.len())
                            .map_or(false, |c| c == '-' || c == '.' || c == ':')
                };

                for (quota_model, &pct) in &t.model_quotas {
                    if pct <= 0 {
                        // Direct match always blocks
                        if quota_model == target_model || quota_model == normalized_target {
                            tracing::info!(
                                "  ⛔ {} - SKIP: Zero quota for exact model '{}' (pct={})",
                                t.email, quota_model, pct
                            );
                            return false;
                        }
                        
                        // Only block if quota_model is MORE specific variant of target
                        // e.g., "gemini-2.5-pro-preview: 0%" blocks "gemini-2.5-pro"
                        // But "gemini-2.5: 0%" does NOT block "gemini-2.5-pro"
                        if is_more_specific_variant(quota_model, target_model)
                            || is_more_specific_variant(quota_model, normalized_target)
                        {
                            tracing::info!(
                                "  ⛔ {} - SKIP: Zero quota for more-specific variant '{}' (target: '{}')",
                                t.email, quota_model, target_model
                            );
                            return false;
                        }
                    }
                }
            }

            true
        });

        // [FIX] Log filtering results for diagnostics
        let filtered_count = initial_count - tokens_snapshot.len();
        if filtered_count > 0 {
            tracing::info!(
                "🔍 [Filter] Model '{}': {} of {} accounts filtered out, {} remaining",
                target_model,
                filtered_count,
                initial_count,
                tokens_snapshot.len()
            );
        }
    }

    /// Apply selected mode filtering
    fn apply_selected_mode_filter(
        &self,
        tokens_snapshot: &mut Vec<ProxyToken>,
        target_model: &str,
        normalized_target: &str,
        scheduling: &crate::proxy::sticky_config::StickySessionConfig,
    ) -> Result<(), String> {
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
                                || m == normalized_target
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
                    *tokens_snapshot = backup;
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

        Ok(())
    }

    /// Try 60-second lock on last used account
    async fn try_60s_lock(
        &self,
        tokens_snapshot: &[ProxyToken],
        attempted: &HashSet<String>,
        normalized_target: &str,
        quota_protection_enabled: bool,
        last_used_account_id: &Option<(String, std::time::Instant)>,
    ) -> Option<ProxyToken> {
        if let Some((account_id, last_time)) = last_used_account_id {
            if last_time.elapsed().as_secs() < 60 && !attempted.contains(account_id) {
                if let Some(found) =
                    tokens_snapshot.iter().find(|t| &t.account_id == account_id)
                {
                    if !self
                        .is_rate_limited(&found.account_id, Some(normalized_target))
                        .await
                        && !(quota_protection_enabled
                            && found.protected_models.contains(normalized_target))
                    {
                        tracing::debug!(
                            "60s Window: Force reusing last account: {}",
                            found.email
                        );
                        return Some(found.clone());
                    }
                }
            }
        }
        None
    }

    /// Select token based on scheduling mode (P2C or Round-Robin)
    async fn select_by_mode(
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

        if scheduling.mode == SchedulingMode::P2C {
            // Pre-filter rate limited accounts for P2C (async context)
            let mut available_for_p2c: Vec<ProxyToken> = Vec::new();
            for t in tokens_snapshot.iter() {
                if !self.is_rate_limited(&t.account_id, Some(normalized_target)).await {
                    available_for_p2c.push(t.clone());
                }
            }

            if let Some(selected) = self.select_with_p2c(
                &available_for_p2c,
                attempted,
                normalized_target,
                quota_protection_enabled,
            ) {
                *need_update_last_used = Some((selected.account_id.clone(), std::time::Instant::now()));
                return Some(selected.clone());
            }
        } else {
            return self
                .select_round_robin(
                    tokens_snapshot,
                    attempted,
                    normalized_target,
                    quota_protection_enabled,
                    session_id,
                    scheduling,
                    need_update_last_used,
                )
                .await;
        }
        None
    }

    /// Try optimistic reset when all accounts are rate-limited
    async fn try_optimistic_reset(
        &self,
        tokens_snapshot: &[ProxyToken],
        attempted: &HashSet<String>,
    ) -> Result<ProxyToken, String> {
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
                    return Ok(t.clone());
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
                        return Ok(t.clone());
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
}
