// Round-Robin Selection Logic

use super::super::manager::TokenManager;
use super::super::models::ProxyToken;
use std::collections::HashSet;
use std::sync::atomic::Ordering;

impl TokenManager {
    /// Select token using round-robin
    pub(crate) async fn select_round_robin(
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
}
