// Scheduling and Session Management

use super::manager::TokenManager;
use crate::proxy::sticky_config::StickySessionConfig;

impl TokenManager {
    /// Get current scheduling configuration
    pub async fn get_sticky_config(&self) -> StickySessionConfig {
        self.sticky_config.read().await.clone()
    }

    /// Update scheduling configuration
    pub async fn update_sticky_config(&self, new_config: StickySessionConfig) {
        let mut config = self.sticky_config.write().await;
        *config = new_config;
        tracing::debug!("Scheduling configuration updated: {:?}", *config);
    }

    /// Clear session binding for a specific session
    #[allow(dead_code)]
    pub fn clear_session_binding(&self, session_id: &str) {
        self.session_accounts.remove(session_id);
    }

    /// Clear all session bindings
    pub fn clear_all_sessions(&self) {
        self.session_accounts.clear();
    }
}
