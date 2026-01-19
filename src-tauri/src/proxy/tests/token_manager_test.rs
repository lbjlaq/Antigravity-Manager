
#[cfg(test)]
mod tests {
    use crate::proxy::token_manager::TokenManager;
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::time::{sleep, Duration};

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
        let (token1, _, _) = tm.get_token("claude", false, None, "claude-3-opus").await.unwrap();
        // Since get_token returns access_token, refresh_token, email. 
        // But get_token internals return account_id? No, get_token returns tuple strings.
        // Let's check token_manager.rs: returns (String, String, String) -> (access, refresh, email) usually?
        // Ah, looking at get_token signature: -> Result<(String, String, String), String>
        // Usually (access_token, refresh_token, email) based on usage context.
        // Let's verify by checking returned email.
        
        // Wait, since we can't easily peek inside DashMap, we rely on returned values.
        // Assuming email format "u1@test.com"
        
        // However, TokenManager usually does round-robin or something.
        // But we implemented sorting! 
        // logic: ULTRA > PRO > FREE.
        
        // Since we have parallel tests potentially (though these act on separate dirs), 
        // and we have 1 thread usually or atomic index.
        
        // The sorted list is rebuilt inside get_token_internal every time?
        // YES: let mut tokens_snapshot: Vec<ProxyToken> = ... .collect(); keys.sort_by ...
        
        // So index logic: 
        // global atomic counter `current_index` increments.
        // But the list is sorted.
        // If sorting is stable (logic is), then [ULTRA, PRO, FREE].
        // index 0 -> ULTRA.
        // index 1 -> PRO.
        // index 2 -> FREE.
        
        // Let's fetch 3 times.
        // Note: get_token calls `fetch_add` -> so it rotates.
        
        // Since `force_rotate` is false, and first call...
        // Wait, "Mode B: 原子化 60s 全局锁定" might kick in if there's a last used account logic properly working?
        // But here we just created TM, last used is None. It will pick from sorted list at index 0.
        
        // Access token is mock_at, verify via email or something?
        // get_token returns (access_token, refresh_token, project_id??) or email?
        // I need to check `src-tauri/src/proxy/token_manager.rs` line 479 signature return type usage.
        // Wait, I read the file. The signature is Result<(String, String, String), String>.
        
        // Since I cannot read all lines in previous context, verification is safer if I guess or standard usage.
        // Let's assume it returns standard auth tuple.
        // To properly assert, I should probably check if I can inspect the email.
        // If the return tuple doesn't have email, I can't verify easily unless I check access token which is constant mock.
        
        // Ah, I created accounts with generated emails. 
        // If get_token returns (at, rt, email) or (at, rt, project_id)?
        // Most proxies need AT/RT. Email is for logging usually.
        // Checking `token.rs`: 
        // `Ok((token.access_token.clone(), token.refresh_token.clone(), token.project_id.clone().unwrap_or_default()))` ??
        // I should check `token_manager.rs` again for the `Ok(...)` return line.
        // I will do that via `view_file` to be safe before writing specific assertions on the tuple.
        
        // BUT, for now, let's write the test assuming generic "get a token" first, 
        // and I will use `token_manager.rs` view to confirm tuple content if I can.
        // Actually, I looked at lines 1-800. `get_token` calls `get_token_internal`.
        // `get_token_internal` returns `match target_token` ...
        // I need to see the return statement of `get_token_internal`. It was likely cut off around line 800.
        // I'll assume I need to read it.
        
        // STRATEGY: 
        // 1. Write the tests with placeholders for assertions or assume (at, rt, email). 
        // 2. Or better, read the file first to be 100% sure. 
        // Since I am in "EXECUTION", I should just do it. I'll read the end of `token_manager.rs` first.
        panic!("Please check the return value first!");
    }
}
