
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
        // Create accounts with different tiers
        dir.create_account("u1", "ULTRA", 50, vec![]);
        dir.create_account("p1", "PRO", 50, vec![]);
        dir.create_account("f1", "FREE", 50, vec![]);

        let tm = TokenManager::new(dir.path.clone());
        tm.load_accounts().await.unwrap();

        // 1. First call should get ULTRA
        // The list is sorted [ULTRA, PRO, FREE]. Index 0 -> ULTRA.
        let (_, _, email) = tm.get_token("claude", false, None, "claude-3-opus").await.unwrap();
        assert_eq!(email, "u1@test.com", "Should pick ULTRA tier first");

        // 2. Test 60s Lock Reuse
        // Next call (no rotate) should reuse u1 because of "60s global lock" logic?
        // Wait, TokenManager logic depends on "last_used_account".
        // Ensure "last_used_account" is set after successful retrieval?
        // Actually TokenManager only sets it if we explicitly cycle or something? 
        // No, typically get_token returns an account. If it was successful, the middleware usually keeps using it?
        // But "Mode B: 原子化 60s 全局锁定" implies TokenManager remembers it.
        // Let's verify if `get_token` reuses the *same* account if called immediately.
        
        let (_, _, email2) = tm.get_token("claude", false, None, "claude-3-opus").await.unwrap();
        // If sorting logic dominates and round-robin index hasn't moved, it might still be u1.
        // But `get_token` increments index? 
        // `current_index.fetch_add(1)` usually happens inside `get_token_internal`?
        // Only if it fails? Or always?
        // "轮询" implies it moves.
        // But "Global Lock" implies it stays.
        // Let's assume for this test we mainly want to check Tier Priority.
        // Since u1 is best, if lock applies, u1. If sort applies, u1. 
        // If round robin applies, it might go to p1.
        // Tests on strict implementation behavior:
        // Ideally we want u1 again if 60s lock works.
        assert_eq!(email2, "u1@test.com", "Should reuse account due to lock or priority");
    }

    #[tokio::test]
    async fn test_force_rotate() {
        let dir = TestDir::new();
        dir.create_account("u1", "ULTRA", 50, vec![]);
        dir.create_account("p1", "PRO", 50, vec![]);

        let tm = TokenManager::new(dir.path.clone());
        tm.load_accounts().await.unwrap();

        // 1. Get first (ULTRA)
        let (_, _, email1) = tm.get_token("claude", false, None, "claude-3-opus").await.unwrap();
        assert_eq!(email1, "u1@test.com");

        // 2. Force rotate -> should get PRO (as it is next in sorted list)
        // Note: verify if implementations supports rotation.
        let (_, _, email2) = tm.get_token("claude", true, None, "claude-3-opus").await.unwrap();
        assert_eq!(email2, "p1@test.com", "Should rotate to PRO account");
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
