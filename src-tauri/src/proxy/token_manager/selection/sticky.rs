// Sticky Session Logic

use super::super::manager::TokenManager;
use super::super::models::ProxyToken;
use std::collections::HashSet;

impl TokenManager {
    /// Try to use sticky session
    pub(crate) async fn try_sticky_session(
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
}
