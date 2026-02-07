// Token Scoring and Sorting Logic

use super::super::manager::TokenManager;
use super::super::models::ProxyToken;
use std::sync::atomic::Ordering;

impl TokenManager {
    /// Sort tokens by priority (tier, health, reset_time, connections, quota)
    pub(crate) fn sort_tokens(&self, tokens: &mut Vec<ProxyToken>) {
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
}
