// Power of 2 Choices (P2C) Selection Algorithm

use super::super::manager::TokenManager;
use super::super::models::ProxyToken;
use std::collections::HashSet;

impl TokenManager {
    /// P2C pool size - select from top N candidates
    pub(crate) const P2C_POOL_SIZE: usize = 5;

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
    pub(crate) fn select_with_p2c<'a>(
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
