use std::sync::Arc;
use crate::proxy::TokenManager;

pub struct HealthChecker;

impl HealthChecker {
    pub async fn should_use_fallback(
        token_manager: &Arc<TokenManager>,
        protocol: &str,
        model: &str,
        auto_switch_back: bool,
    ) -> bool {
        if !auto_switch_back {
            return false;
        }
        
        let has_available = token_manager.has_available_account(protocol, model).await;
        !has_available
    }
}
