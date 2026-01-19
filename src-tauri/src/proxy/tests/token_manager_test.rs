
#[cfg(test)]
mod tests {
    use crate::proxy::token_manager::TokenManager;
    use std::fs;
    use std::path::PathBuf;

    // Helper to create a temporary test directory
    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("antigravity_test_{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            fs::create_dir_all(path.join("accounts")).unwrap();
            Self { path }
        }

        fn create_account(&self, id: &str, tier: &str, quota_pct: i32, protected: Vec<&str>) {
            let account_json = serde_json::json!({
                "id": id,
                "email": format!("{}@test.com", id),
                "token": {
                    "access_token": "mock_at",
                    "refresh_token": "mock_rt",
                    "project_id": "mock-project-id",
                    "expires_in": 3600,
                    "expiry_timestamp": chrono::Utc::now().timestamp() + 3600
                },
                "quota": {
                    "subscription_tier": tier,
                    "models": [ 
                        { "name": "claude-3-opus", "percentage": quota_pct } 
                    ]
                },
                "protected_models": protected
            });

            let file_path = self.path.join("accounts").join(format!("{}.json", id));
            fs::write(file_path, account_json.to_string()).unwrap();
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[tokio::test]
    async fn test_tier_priority() {
        let dir = TestDir::new();
        // Create accounts with different tiers (order of creation doesn't matter, they are sorted)
        dir.create_account("f1", "FREE", 50, vec![]);
        dir.create_account("p1", "PRO", 50, vec![]);
        dir.create_account("u1", "ULTRA", 50, vec![]);

        let tm = TokenManager::new(dir.path.clone());
        tm.load_accounts().await.unwrap();

        // TokenManager sorts accounts by tier: ULTRA > PRO > FREE
        // First call should always pick ULTRA (highest priority)
        let (_, _, email) = tm.get_token("claude", false, None, "claude-3-opus").await.unwrap();
        assert_eq!(email, "u1@test.com", "Should pick ULTRA tier first due to sorting");

        // Second call (no force_rotate): In Balance mode, 60s lock reuses last account
        // This tests that the same top-priority account is returned when no rotation needed
        let (_, _, email2) = tm.get_token("claude", false, None, "claude-3-opus").await.unwrap();
        assert_eq!(email2, "u1@test.com", "Should reuse same account (60s lock or same priority)");
    }

    #[tokio::test]
    async fn test_force_rotate() {
        let dir = TestDir::new();
        dir.create_account("u1", "ULTRA", 50, vec![]);
        dir.create_account("p1", "PRO", 50, vec![]);

        let tm = TokenManager::new(dir.path.clone());
        tm.load_accounts().await.unwrap();

        // 1. Get first account (should be ULTRA due to tier priority)
        let (_, _, email1) = tm.get_token("claude", false, None, "claude-3-opus").await.unwrap();
        assert_eq!(email1, "u1@test.com", "First call should pick ULTRA tier");

        // 2. Force rotate: should skip the last-used account and pick next available
        // In Mode C (round-robin), it iterates from current_index
        let (_, _, email2) = tm.get_token("claude", true, None, "claude-3-opus").await.unwrap();
        
        // Force rotate should give us a DIFFERENT account (either p1 or wrap back to u1)
        // The key assertion: force_rotate SHOULD switch away from the locked account
        // If only 2 accounts exist, the second one must be different from the first
        assert_ne!(email1, email2, "Force rotate should switch to a different account");
    }

    #[tokio::test]
    async fn test_quota_protection() {
        // NOTE: This test depends on `quota_protection.enabled` from the user's config file.
        // In CI or fresh environments, this may default to `false`, causing the test to behave differently.
        // We check the actual config state and adjust assertions accordingly.
        
        let quota_protection_enabled = crate::modules::config::load_app_config()
            .map(|cfg| cfg.quota_protection.enabled)
            .unwrap_or(false);
        
        let dir = TestDir::new();
        // u1 has claude-3-opus in protected_models
        dir.create_account("u1", "ULTRA", 10, vec!["claude-3-opus"]);
        // u2 is free but valid for this model
        dir.create_account("u2", "FREE", 50, vec![]); 

        let tm = TokenManager::new(dir.path.clone());
        tm.load_accounts().await.unwrap();
        
        // Request for claude-3-opus
        let (_, _, email) = tm.get_token("claude", false, None, "claude-3-opus").await.unwrap();
        
        if quota_protection_enabled {
            // When enabled, u1 should be skipped due to protection
            assert_eq!(email, "u2@test.com", "Should skip u1 because it is protected for claude-3-opus");
        } else {
            // When disabled, u1 wins due to tier priority (ULTRA > FREE)
            assert_eq!(email, "u1@test.com", "Quota protection disabled, tier priority wins");
        }
        
        // Request for another model (e.g. gpt-4) - u1 is NOT protected for this model.
        // u1 is ULTRA, u2 is FREE. Priority -> u1.
        let (_, _, email2) = tm.get_token("claude", false, None, "gpt-4").await.unwrap();
        assert_eq!(email2, "u1@test.com", "Should pick u1 for non-protected model due to tier priority");
    }
}
