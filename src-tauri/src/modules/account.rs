use serde::Serialize;
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

use crate::models::account::{classify_lock_reason, forbidden_model_scope, AccountLockKind};
use crate::models::{
    Account, AccountBanInfo, AccountIndex, AccountSummary, DeviceProfile, DeviceProfileVersion,
    QuotaData, TokenData,
};
use crate::modules;
use once_cell::sync::Lazy;
use std::sync::{Mutex, OnceLock, RwLock};

/// Global per-account lock to prevent concurrent write collisions on the same account JSON file
static ACCOUNT_FILE_LOCKS: Lazy<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn get_account_lock(account_id: &str) -> Arc<Mutex<()>> {
    let mut locks = ACCOUNT_FILE_LOCKS.lock().unwrap();
    locks
        .entry(account_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

#[cfg(test)]
pub(crate) static SHARED_TEST_ENV_LOCK: Lazy<std::sync::Mutex<()>> =
    Lazy::new(|| std::sync::Mutex::new(()));

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    static TEST_DIR_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

    struct TestDataDir {
        path: PathBuf,
    }

    impl TestDataDir {
        fn new() -> Self {
            let seq = TEST_DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let temp_path = std::env::temp_dir().join(format!(
                "antigravity_test_{}_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                seq
            ));
            fs::create_dir_all(&temp_path).expect("Failed to create temp dir");

            Self { path: temp_path }
        }

        fn path(&self) -> &PathBuf {
            &self.path
        }
    }

    impl Drop for TestDataDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    /// Helper to write corrupted content to accounts.json
    fn write_corrupted_index(path: &PathBuf, content: &[u8]) {
        let index_path = path.join("accounts.json");
        fs::write(&index_path, content).expect("Failed to write corrupted index");
    }

    /// Helper to create a valid account file in accounts/ directory
    fn create_account_file(path: &PathBuf, account_id: &str, email: &str) {
        let accounts_dir = path.join("accounts");
        fs::create_dir_all(&accounts_dir).expect("Failed to create accounts dir");

        let account = Account::new(
            account_id.to_string(),
            email.to_string(),
            TokenData::new(
                "test_access_token".to_string(),
                "test_refresh_token".to_string(),
                3600,
                Some(email.to_string()),
                None,
                None,
                true,
                None,
            ),
        );

        let content = serde_json::to_string_pretty(&account).expect("Failed to serialize account");
        let account_path = accounts_dir.join(format!("{}.json", account_id));
        fs::write(&account_path, content).expect("Failed to write account file");
    }

    #[test]
    fn test_normalize_data_dir_path_strips_windows_prefix() {
        assert_eq!(
            format_data_dir_path(Path::new(r"\\?\F:\antigravity-tools-data")),
            r"F:\antigravity-tools-data"
        );
        assert_eq!(
            format_data_dir_path(Path::new(r"\\?\UNC\server\share\data")),
            r"\\server\share\data"
        );
        assert_eq!(format_data_dir_path(Path::new("//?/C:/data")), "C:/data");
        assert_eq!(format_data_dir_path(Path::new("/app/data")), "/app/data");
        assert_eq!(
            format_data_dir_path(Path::new(r"F:\antigravity-tools-data")),
            r"F:\antigravity-tools-data"
        );
    }

    #[test]
    fn test_resolve_account_summary_by_index_email_name_and_round_robin() {
        let accounts = vec![
            crate::models::AccountSummary {
                id: "id-alpha-111".to_string(),
                email: "alpha@test.com".to_string(),
                name: Some("工作号".to_string()),
                disabled: false,
                proxy_disabled: false,
                protected_models: HashSet::new(),
                created_at: 1,
                last_used: 1,
            },
            crate::models::AccountSummary {
                id: "id-beta-222".to_string(),
                email: "beta@test.com".to_string(),
                name: Some("个人号".to_string()),
                disabled: false,
                proxy_disabled: false,
                protected_models: HashSet::new(),
                created_at: 2,
                last_used: 2,
            },
            crate::models::AccountSummary {
                id: "id-gamma-333".to_string(),
                email: "gamma@test.com".to_string(),
                name: None,
                disabled: true, // 已禁用
                proxy_disabled: false,
                protected_models: HashSet::new(),
                created_at: 3,
                last_used: 3,
            },
        ];

        let index = crate::models::AccountIndex {
            version: "1.0".to_string(),
            accounts,
            current_account_id: Some("id-alpha-111".to_string()),
            current_target_ide: Some("agy".to_string()),
            current_classic_account_id: None,
            current_ide_account_id: None,
            current_agy_account_id: Some("id-alpha-111".to_string()),
            current_acp_account_id: None,
        };

        // 1. 纯数字序号 1-based
        let acc1 = resolve_account_summary_in_index(&index, Some("2"), None).unwrap();
        assert_eq!(acc1.id, "id-beta-222");

        // 2. 精确匹配 Email
        let acc2 = resolve_account_summary_in_index(&index, Some("alpha@test.com"), None).unwrap();
        assert_eq!(acc2.id, "id-alpha-111");

        // 3. 别名 Name 匹配
        let acc3 = resolve_account_summary_in_index(&index, Some("工作号"), None).unwrap();
        assert_eq!(acc3.id, "id-alpha-111");

        // 4. 模糊包含匹配
        let acc4 = resolve_account_summary_in_index(&index, Some("beta"), None).unwrap();
        assert_eq!(acc4.id, "id-beta-222");

        // 5. 无参顺延轮替：当前是 alpha，顺延挑出 beta（跳过禁用的 gamma）
        let acc5 = resolve_account_summary_in_index(&index, None, Some("agy")).unwrap();
        assert_eq!(acc5.id, "id-beta-222");

        // 6. 当前若是 beta，顺延跨过 gamma 回到 alpha
        let mut index_at_beta = index.clone();
        index_at_beta.current_agy_account_id = Some("id-beta-222".to_string());
        let acc6 = resolve_account_summary_in_index(&index_at_beta, None, Some("agy")).unwrap();
        assert_eq!(acc6.id, "id-alpha-111");
    }

    #[test]
    fn test_migrate_data_dir_rename_and_copy() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let previous_env = std::env::var("ABV_DATA_DIR").ok();
        let previous_pointer_env = std::env::var("ABV_DATA_DIR_POINTER_FILE").ok();

        // 记录真实家目录指针，结尾断言它自始至终没被改动。
        // 背景：`migrate_data_dir` 会写数据目录指针。如果测试让它写到真实的
        // `~/.antigravity_tools_location`，那么测试一旦被中断（Ctrl-C / 超时 /
        // 进程被杀），恢复逻辑不会执行，用户的数据目录就会被永久指向 /tmp 下的
        // 临时目录 —— 应用下次启动会读到空数据目录，表现为「账号全部消失」。
        let real_pointer = dirs::home_dir()
            .expect("home")
            .join(".antigravity_tools_location");
        let real_pointer_before = fs::read_to_string(&real_pointer).ok();

        let src = TestDataDir::new();
        fs::write(src.path().join("marker.txt"), "hello").unwrap();

        let dest_parent = TestDataDir::new();
        let dest = dest_parent.path().join("moved_data");
        // 指针文件也放进临时目录（复用已存在的 dest_parent，避免多建一个
        // 时间戳目录而可能与 src 撞名）
        let pointer_path = dest_parent.path().join("location");

        std::env::set_var("ABV_DATA_DIR", src.path());
        std::env::set_var("ABV_DATA_DIR_POINTER_FILE", &pointer_path);

        let restore = || {
            match &previous_env {
                Some(value) => std::env::set_var("ABV_DATA_DIR", value),
                None => std::env::remove_var("ABV_DATA_DIR"),
            }
            match &previous_pointer_env {
                Some(value) => std::env::set_var("ABV_DATA_DIR_POINTER_FILE", value),
                None => std::env::remove_var("ABV_DATA_DIR_POINTER_FILE"),
            }
            if let Ok(mut guard) = data_dir_override_slot().write() {
                *guard = None;
            }
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let resolved = migrate_data_dir(dest.clone()).unwrap();
            assert!(resolved.join("marker.txt").exists());
            assert!(!src.path().join("marker.txt").exists());
            assert_eq!(
                fs::read_to_string(resolved.join("marker.txt")).unwrap(),
                "hello"
            );
            let shown = format_data_dir_path(&resolved);
            assert!(
                !shown.contains(r"\\?\"),
                "migrated path must not keep Windows verbatim prefix: {shown}"
            );
            // 指针必须写在被重定向后的临时位置
            assert_eq!(
                fs::read_to_string(&pointer_path).unwrap().trim(),
                format_data_dir_path(&resolved)
            );
        }));

        let real_pointer_after = fs::read_to_string(&real_pointer).ok();
        restore();

        assert_eq!(
            real_pointer_before, real_pointer_after,
            "测试污染了真实的 ~/.antigravity_tools_location！"
        );

        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    #[test]
    fn test_load_account_index_with_bom_prefix() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();

        // UTF-8 BOM followed by valid JSON
        let bom = [0xEF, 0xBB, 0xBF];
        let json = r#"{"version":"2.0","accounts":[],"current_account_id":null}"#;
        let mut content = Vec::new();
        content.extend_from_slice(&bom);
        content.extend_from_slice(json.as_bytes());

        write_corrupted_index(dir.path(), &content);

        let result = load_account_index_in_dir(dir.path());

        // New behavior: BOM is stripped and JSON parses successfully
        assert!(
            result.is_ok(),
            "BOM should be stripped and JSON should parse: {:?}",
            result
        );
        let index = result.unwrap();
        assert!(index.accounts.is_empty());
        println!("BOM case: successfully loaded index after sanitization");
    }

    #[test]
    fn test_load_account_index_with_nul_prefix() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();

        // NUL byte prefix followed by valid JSON
        let nul = [0x00];
        let json = r#"{"version":"2.0","accounts":[],"current_account_id":null}"#;
        let mut content = Vec::new();
        content.extend_from_slice(&nul);
        content.extend_from_slice(json.as_bytes());

        write_corrupted_index(dir.path(), &content);

        let result = load_account_index_in_dir(dir.path());

        // New behavior: NUL bytes are stripped and JSON parses successfully
        assert!(
            result.is_ok(),
            "NUL prefix should be stripped and JSON should parse: {:?}",
            result
        );
        let index = result.unwrap();
        assert!(index.accounts.is_empty());
        println!("NUL prefix case: successfully loaded index after sanitization");
    }

    #[test]
    fn test_load_account_index_with_garbage_content() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();

        // Non-JSON garbage content - should trigger recovery
        write_corrupted_index(dir.path(), b"\0\0not json");

        let result = load_account_index_in_dir(dir.path());

        // New behavior: garbage content triggers recovery, returns empty index
        assert!(
            result.is_ok(),
            "Garbage content should trigger recovery and return Ok: {:?}",
            result
        );
        let index = result.unwrap();
        assert!(
            index.accounts.is_empty(),
            "Recovered index should be empty when no account files exist"
        );
        println!("Garbage content case: successfully recovered to empty index");
    }

    #[test]
    fn test_load_account_index_with_empty_file() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();

        // Empty file
        write_corrupted_index(dir.path(), b"");

        let result = load_account_index_in_dir(dir.path());

        // Current behavior: empty file returns new empty index
        assert!(result.is_ok());
        let index = result.unwrap();
        assert!(index.accounts.is_empty());
    }

    #[test]
    fn test_load_account_index_with_whitespace_only() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();

        // Whitespace-only file
        write_corrupted_index(dir.path(), b"   \n\t  ");

        let result = load_account_index_in_dir(dir.path());

        // Current behavior: whitespace-only file returns new empty index
        assert!(result.is_ok());
        let index = result.unwrap();
        assert!(index.accounts.is_empty());
    }

    #[test]
    fn test_missing_index_with_existing_accounts() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();

        // Create accounts directory with account files but NO accounts.json index
        create_account_file(dir.path(), "test-id-1", "user1@example.com");
        create_account_file(dir.path(), "test-id-2", "user2@example.com");

        // accounts.json does not exist
        let index_path = dir.path().join("accounts.json");
        assert!(!index_path.exists());

        // Load account index - should recover from accounts directory
        let result = load_account_index_in_dir(dir.path());
        assert!(result.is_ok(), "Should recover from accounts directory");
        let index = result.unwrap();
        assert_eq!(
            index.accounts.len(),
            2,
            "Index should have 2 accounts recovered from accounts directory"
        );

        // Verify recovered accounts have correct data
        let emails: Vec<_> = index.accounts.iter().map(|s| s.email.clone()).collect();
        assert!(emails.contains(&"user1@example.com".to_string()));
        assert!(emails.contains(&"user2@example.com".to_string()));

        // Verify account files still exist
        let accounts_dir = dir.path().join("accounts");
        let account_files: Vec<_> = fs::read_dir(&accounts_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
            .collect();
        assert_eq!(
            account_files.len(),
            2,
            "Account files should still exist on disk"
        );

        println!(
            "Missing index with existing accounts: successfully recovered {} accounts",
            index.accounts.len()
        );
    }

    #[test]
    fn test_save_account_index_roundtrip() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();

        // Build an AccountIndex with 2 accounts
        let now = chrono::Utc::now().timestamp();
        let index = AccountIndex {
            version: "2.0".to_string(),
            accounts: vec![
                AccountSummary {
                    id: "acc-1".to_string(),
                    email: "user1@example.com".to_string(),
                    name: Some("User One".to_string()),
                    disabled: false,
                    proxy_disabled: false,
                    protected_models: HashSet::new(),
                    created_at: now,
                    last_used: now,
                },
                AccountSummary {
                    id: "acc-2".to_string(),
                    email: "user2@example.com".to_string(),
                    name: None,
                    disabled: true,
                    proxy_disabled: true,
                    protected_models: HashSet::new(),
                    created_at: now - 100,
                    last_used: now - 50,
                },
            ],
            current_account_id: Some("acc-1".to_string()),
            current_target_ide: None,
            current_classic_account_id: None,
            current_ide_account_id: None,
            current_agy_account_id: None,
            current_acp_account_id: None,
        };

        // Save the index
        save_account_index_in_dir(dir.path(), &index).expect("Failed to save account index");

        // Load it back
        let loaded = load_account_index_in_dir(dir.path()).expect("Failed to load account index");

        // Assert it matches
        assert_eq!(loaded.accounts.len(), 2, "Should have 2 accounts");
        assert_eq!(
            loaded.current_account_id,
            Some("acc-1".to_string()),
            "current_account_id should match"
        );

        // Check first account
        let acc1 = loaded
            .accounts
            .iter()
            .find(|a| a.id == "acc-1")
            .expect("acc-1 should exist");
        assert_eq!(acc1.email, "user1@example.com");
        assert_eq!(acc1.name, Some("User One".to_string()));
        assert!(!acc1.disabled);
        assert!(!acc1.proxy_disabled);

        // Check second account
        let acc2 = loaded
            .accounts
            .iter()
            .find(|a| a.id == "acc-2")
            .expect("acc-2 should exist");
        assert_eq!(acc2.email, "user2@example.com");
        assert_eq!(acc2.name, None);
        assert!(acc2.disabled);
        assert!(acc2.proxy_disabled);

        println!(
            "save_account_index roundtrip: successfully saved and loaded index with {} accounts",
            loaded.accounts.len()
        );
    }

    #[test]
    fn test_set_current_account_id_with_target() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        // Create a dummy account index with some accounts
        let now = chrono::Utc::now().timestamp();
        let index = AccountIndex {
            version: "2.0".to_string(),
            accounts: vec![AccountSummary {
                id: "acc-1".to_string(),
                email: "user1@example.com".to_string(),
                name: Some("User One".to_string()),
                disabled: false,
                proxy_disabled: false,
                protected_models: HashSet::new(),
                created_at: now,
                last_used: now,
            }],
            current_account_id: None,
            current_target_ide: None,
            current_classic_account_id: None,
            current_ide_account_id: None,
            current_agy_account_id: None,
            current_acp_account_id: None,
        };
        save_account_index_in_dir(dir.path(), &index).unwrap();

        // 1. Call set_current_account_id_with_target with Some("agy")
        set_current_account_id_with_target("acc-1", Some("agy")).unwrap();

        // Load back and verify
        let index = load_account_index_in_dir(dir.path()).unwrap();
        assert_eq!(index.current_account_id, Some("acc-1".to_string()));
        assert_eq!(index.current_target_ide, Some("agy".to_string()));

        // 2. Call set_current_account_id (which sets target to None)
        set_current_account_id("acc-1").unwrap();

        // Load back and verify target is None
        let index = load_account_index_in_dir(dir.path()).unwrap();
        assert_eq!(index.current_account_id, Some("acc-1".to_string()));
        assert_eq!(index.current_target_ide, None);

        // Clean up environment variable
        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_toggle_proxy_status_synchronizes_index_and_account_file() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        create_account_file(dir.path(), "acc-toggle", "toggle@example.com");
        let now = chrono::Utc::now().timestamp();
        let index = AccountIndex {
            version: "2.0".to_string(),
            accounts: vec![AccountSummary {
                id: "acc-toggle".to_string(),
                email: "toggle@example.com".to_string(),
                name: Some("Toggle User".to_string()),
                disabled: false,
                proxy_disabled: false,
                protected_models: HashSet::new(),
                created_at: now,
                last_used: now,
            }],
            current_account_id: Some("acc-toggle".to_string()),
            current_target_ide: None,
            current_classic_account_id: None,
            current_ide_account_id: None,
            current_agy_account_id: None,
            current_acp_account_id: None,
        };
        save_account_index_in_dir(dir.path(), &index).unwrap();

        // 1. Disable proxy
        toggle_proxy_status("acc-toggle", false, Some("testing disable")).unwrap();

        let loaded_account = load_account("acc-toggle").unwrap();
        assert!(loaded_account.proxy_disabled);
        assert_eq!(
            loaded_account.proxy_disabled_reason.as_deref(),
            Some("testing disable")
        );

        let loaded_index = load_account_index_in_dir(dir.path()).unwrap();
        assert!(
            loaded_index
                .accounts
                .iter()
                .find(|a| a.id == "acc-toggle")
                .unwrap()
                .proxy_disabled,
            "Index must be synchronized when proxy is disabled"
        );

        // 2. Re-enable proxy
        toggle_proxy_status("acc-toggle", true, None).unwrap();

        let reloaded_account = load_account("acc-toggle").unwrap();
        assert!(!reloaded_account.proxy_disabled);
        assert!(reloaded_account.proxy_disabled_reason.is_none());

        let reloaded_index = load_account_index_in_dir(dir.path()).unwrap();
        assert!(
            !reloaded_index
                .accounts
                .iter()
                .find(|a| a.id == "acc-toggle")
                .unwrap()
                .proxy_disabled,
            "Index must be synchronized when proxy is re-enabled"
        );

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_toggle_proxy_status_rejects_system_locked_account() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "acc-system-lock";
        create_account_file(dir.path(), acc_id, "system-lock@example.com");

        // 403 锁：手动启用必须被拒绝，且原状态保持不变。
        let mut account = load_account(acc_id).unwrap();
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("Forbidden (403): Location not supported".into());
        let mut q = crate::models::QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some("Forbidden (403): Location not supported".into());
        account.quota = Some(q);
        save_account(&account).unwrap();

        let err = toggle_proxy_status(acc_id, true, None).unwrap_err();
        assert!(
            err.contains("403/validation lock"),
            "unexpected error: {}",
            err
        );
        let locked = load_account(acc_id).unwrap();
        assert!(locked.proxy_disabled);
        assert!(locked.quota.as_ref().unwrap().is_forbidden);

        // 验证锁：手动启用同样应被拒绝。
        let mut account = load_account(acc_id).unwrap();
        account.quota = None;
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("validation_required".into());
        account.validation_blocked = true;
        account.validation_blocked_until = Some(chrono::Utc::now().timestamp() + 600);
        save_account(&account).unwrap();

        let err = toggle_proxy_status(acc_id, true, None).unwrap_err();
        assert!(
            err.contains("validation-blocked"),
            "unexpected error: {}",
            err
        );
        let still_blocked = load_account(acc_id).unwrap();
        assert!(still_blocked.proxy_disabled);
        assert!(still_blocked.validation_blocked);

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_list_accounts_synchronizes_index_drift_from_entity() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        create_account_file(dir.path(), "acc-drift", "drift@example.com");
        let now = chrono::Utc::now().timestamp();
        let index = AccountIndex {
            version: "2.0".to_string(),
            accounts: vec![AccountSummary {
                id: "acc-drift".to_string(),
                email: "drift@example.com".to_string(),
                name: Some("Drift User".to_string()),
                disabled: true,
                proxy_disabled: true,
                protected_models: HashSet::new(),
                created_at: now,
                last_used: now,
            }],
            current_account_id: Some("acc-drift".to_string()),
            current_target_ide: None,
            current_classic_account_id: None,
            current_ide_account_id: None,
            current_agy_account_id: None,
            current_acp_account_id: None,
        };
        save_account_index_in_dir(dir.path(), &index).unwrap();

        let accounts = list_accounts().unwrap();
        assert_eq!(accounts.len(), 1);
        assert!(
            !accounts[0].disabled,
            "Account must remain enabled as in entity file"
        );
        assert!(
            !accounts[0].proxy_disabled,
            "Account proxy must remain enabled as in entity file"
        );

        let loaded_index = load_account_index_in_dir(dir.path()).unwrap();
        let summary = loaded_index
            .accounts
            .iter()
            .find(|a| a.id == "acc-drift")
            .unwrap();
        assert!(
            !summary.disabled,
            "Index disabled flag must be synced from entity"
        );
        assert!(
            !summary.proxy_disabled,
            "Index proxy_disabled flag must be synced from entity"
        );

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_backup_created_on_parse_failure() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();

        // Create a valid account file
        create_account_file(dir.path(), "recovered-acc", "recovered@example.com");

        // Create corrupt accounts.json with garbage (non-empty)
        let garbage_content = b"this is not valid json { broken";
        write_corrupted_index(dir.path(), garbage_content);

        // Verify accounts.json exists and is corrupt
        let index_path = dir.path().join("accounts.json");
        assert!(index_path.exists(), "accounts.json should exist");

        // Call load_account_index to trigger recovery and backup creation
        let recovered =
            load_account_index_in_dir(dir.path()).expect("Should recover from accounts");
        assert_eq!(recovered.accounts.len(), 1, "Should recover 1 account");
        assert_eq!(recovered.accounts[0].email, "recovered@example.com");
        assert_eq!(
            recovered.current_account_id,
            Some("recovered-acc".to_string())
        );

        // Assert a backup file exists with prefix "accounts.json.corrupt-"
        let data_dir = dir.path();
        let backup_files: Vec<_> = fs::read_dir(data_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map_or(false, |name| name.starts_with("accounts.json.corrupt-"))
            })
            .collect();

        assert_eq!(backup_files.len(), 1, "Should have exactly one backup file");

        // Verify backup contains the original garbage content
        let backup_content =
            fs::read(&backup_files[0].path()).expect("Should be able to read backup file");
        assert_eq!(
            backup_content, garbage_content,
            "Backup should contain original corrupt content"
        );

        println!("Backup creation on parse failure: successfully created backup");
    }

    #[test]
    fn test_load_account_with_trailing_characters() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();

        create_account_file(dir.path(), "corrupt-tail-acc", "tail@example.com");
        let account_path = dir.path().join("accounts").join("corrupt-tail-acc.json");

        // Append trailing '}' to simulate Issue #3345
        let mut raw = fs::read_to_string(&account_path).unwrap();
        raw.push('}');
        fs::write(&account_path, &raw).unwrap();

        // Load account should successfully self-heal and return valid Account
        let loaded =
            load_account_at_path(&account_path).expect("Should self-heal trailing characters");
        assert_eq!(loaded.id, "corrupt-tail-acc");
        assert_eq!(loaded.email, "tail@example.com");

        // Verify the file was cleaned and re-written as valid JSON
        let healed_raw = fs::read_to_string(&account_path).unwrap();
        let regular_parse: Result<Account, _> = serde_json::from_str(&healed_raw);
        assert!(
            regular_parse.is_ok(),
            "Healed file should be standard valid JSON"
        );
    }

    #[test]
    fn task_quota_refresh_keeps_unexpired_live_limit() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        let account_id = "live-limit-account";
        create_account_file(dir.path(), account_id, "live-limit@example.com");
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let now = chrono::Utc::now().timestamp();
        let mut account = load_account(account_id).unwrap();
        account.live_limited_models.insert(
            "gemini-3-pro-image".to_string(),
            crate::models::account::LiveLimitStatus {
                model: "gemini-3-pro-image".to_string(),
                status: 429,
                reason: "QuotaExhausted".to_string(),
                until: now + 7200,
                detected_at: now,
                message: Some(
                    r#"{"error":{"details":[{"reason":"QUOTA_EXHAUSTED","metadata":{"quotaResetDelay":"2h"}}]}}"#
                        .to_string(),
                ),
            },
        );
        account.live_limited_models.insert(
            "gemini-3.1-flash-image".to_string(),
            crate::models::account::LiveLimitStatus {
                model: "gemini-3.1-flash-image".to_string(),
                status: 429,
                reason: "QuotaExhausted".to_string(),
                until: now + 7200,
                detected_at: now,
                message: Some("QUOTA_EXHAUSTED".to_string()),
            },
        );
        account.live_limited_models.insert(
            "gemini-2.5-pro".to_string(),
            crate::models::account::LiveLimitStatus {
                model: "gemini-2.5-pro".to_string(),
                status: 429,
                reason: "QuotaExhausted".to_string(),
                until: now + 7200,
                detected_at: now,
                message: Some("QUOTA_EXHAUSTED; reset after 2h".to_string()),
            },
        );
        save_account(&account).unwrap();

        let quota: QuotaData = serde_json::from_value(serde_json::json!({
            "models": [
                {"name": "gemini-3-pro-image", "percentage": 99, "reset_time": ""},
                {"name": "gemini-3.1-flash-image", "percentage": 99, "reset_time": ""},
                {"name": "gemini-2.5-pro", "percentage": 99, "reset_time": ""}
            ],
            "last_updated": now
        }))
        .unwrap();
        update_account_quota(account_id, quota).unwrap();

        let updated = load_account(account_id).unwrap();
        assert!(updated
            .live_limited_models
            .contains_key("gemini-3-pro-image"));
        assert!(!updated
            .live_limited_models
            .contains_key("gemini-3.1-flash-image"));
        assert!(!updated.live_limited_models.contains_key("gemini-2.5-pro"));
        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_delete_account_cleans_orphan_file_when_not_in_index() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        // 创建孤儿实体文件，但 accounts.json 索引中不包含它
        let orphan_id = "orphan-ghost-acc-id";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, orphan_id, "orphan@example.com");

        let index = AccountIndex::new();
        save_account_index_in_dir(&path_buf, &index).unwrap();

        let orphan_file = dir
            .path()
            .join("accounts")
            .join(format!("{}.json", orphan_id));
        assert!(orphan_file.exists());

        // 执行 delete_account 即使不在索引中也成功清理物理文件
        let res = delete_account(orphan_id);
        assert!(res.is_ok());
        assert!(!orphan_file.exists());

        // 再次删除不存在的文件和索引返回 Err
        assert!(delete_account("completely-non-existent").is_err());

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_clear_account_forbidden_and_validation_banned_protection() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-banned-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "banned@example.com");

        // 构造真正的 TOS 封禁账号
        let mut account = load_account(acc_id).unwrap();
        account.banned = true;
        account.banned_code = Some("TOS_VIOLATION".to_string());
        account.banned_reason = Some("Terms of Service violation".to_string());
        account.ban_appeal_url = Some("https://support.google.com/appeal".to_string());
        account.validation_url = Some("https://support.google.com/appeal".to_string());
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("banned: TOS_VIOLATION".to_string());
        let mut q = crate::models::QuotaData::new();
        q.is_forbidden = true;
        account.quota = Some(q);
        save_account(&account).unwrap();

        // 尝试通过测活解封
        let res = clear_account_forbidden_and_validation(acc_id, Some("gemini-2.5-flash")).unwrap();
        assert!(!res, "TOS 封禁账号绝不能被探活流程自动解封");

        // 状态必须完整保留
        let updated = load_account(acc_id).unwrap();
        assert!(updated.banned);
        assert_eq!(updated.banned_code.as_deref(), Some("TOS_VIOLATION"));
        assert!(updated.ban_appeal_url.is_some());
        assert!(updated.validation_url.is_some());
        assert!(updated.proxy_disabled);

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_clear_account_forbidden_and_validation_pure_is_forbidden() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-pure-forbidden-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "pure_forbidden@example.com");

        // 构造仅有 quota.is_forbidden = true，无 validation_blocked 与 proxy_disabled 的账号
        let mut account = load_account(acc_id).unwrap();
        account.validation_blocked = false;
        account.proxy_disabled = false;
        let mut q = crate::models::QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some("Forbidden (403): Location not supported".to_string());
        account.quota = Some(q);
        save_account(&account).unwrap();

        // 调用解封
        let recovered =
            clear_account_forbidden_and_validation(acc_id, Some("gemini-2.5-flash")).unwrap();
        assert!(recovered, "纯 is_forbidden 账号应被正确解封");

        let updated = load_account(acc_id).unwrap();
        assert!(!updated.quota.as_ref().unwrap().is_forbidden);
        assert!(updated.quota.as_ref().unwrap().forbidden_reason.is_none());

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_clear_account_forbidden_and_validation_model_isolation() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-model-isolation-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "model_iso@example.com");

        // 构造因调用 Claude 3.5 触发 403 的账号
        let mut account = load_account(acc_id).unwrap();
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some(
            "Forbidden (403): User not authorized to access model claude-3-5-sonnet".to_string(),
        );
        let mut q = crate::models::QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some(
            "Forbidden (403): User not authorized to access model claude-3-5-sonnet".to_string(),
        );
        account.quota = Some(q);
        save_account(&account).unwrap();

        // 1. 使用 gemini-2.5-flash 测活：由于 403 锁定原因绑定在 Claude 模型，测试不同模型不得解封
        let res_gemini =
            clear_account_forbidden_and_validation(acc_id, Some("gemini-2.5-flash")).unwrap();
        assert!(!res_gemini, "测试不同模型绝不能解封特定模型的 403 锁定");

        let still_locked = load_account(acc_id).unwrap();
        assert!(still_locked.proxy_disabled);
        assert!(still_locked.quota.as_ref().unwrap().is_forbidden);

        // 2. 使用 claude-3-5-sonnet 测活成功：测试同一模型，允许解封
        let res_claude =
            clear_account_forbidden_and_validation(acc_id, Some("claude-3-5-sonnet")).unwrap();
        assert!(res_claude, "测试同一受限模型成功时应解除锁定");

        let unlocked = load_account(acc_id).unwrap();
        assert!(!unlocked.proxy_disabled);
        assert!(!unlocked.quota.as_ref().unwrap().is_forbidden);

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_clear_account_forbidden_and_validation_uses_specific_quota_reason() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-generic-proxy-specific-quota";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "specific_quota@example.com");

        // proxy_disabled_reason 是泛化原因，真正的模型绑定信息在 quota.forbidden_reason
        let mut account = load_account(acc_id).unwrap();
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("Forbidden (403)".to_string());
        let mut q = crate::models::QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some(
            "Forbidden (403): User not authorized to access model claude-3-5-sonnet".to_string(),
        );
        account.quota = Some(q);
        save_account(&account).unwrap();

        // 测试 Gemini：必须读取更具体的 quota reason，不能因泛化 proxy reason 误解封
        let res_gemini =
            clear_account_forbidden_and_validation(acc_id, Some("gemini-2.5-flash")).unwrap();
        assert!(
            !res_gemini,
            "泛化 proxy reason 不得掩盖 quota 中的模型绑定信息"
        );
        let still_locked = load_account(acc_id).unwrap();
        assert!(still_locked.proxy_disabled);
        assert!(still_locked.quota.as_ref().unwrap().is_forbidden);

        // 测试同一 Claude 模型：允许正常解封
        let res_claude =
            clear_account_forbidden_and_validation(acc_id, Some("claude-3-5-sonnet")).unwrap();
        assert!(res_claude, "测试匹配的受限模型时应允许解封");

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_clear_account_forbidden_and_validation_validation_blocked() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-validation-blocked-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "validation@example.com");

        // 构造人机验证锁定的账号
        let mut account = load_account(acc_id).unwrap();
        account.validation_blocked = true;
        account.validation_blocked_reason = Some("VALIDATION_REQUIRED: Human captcha".to_string());
        account.validation_url = Some("https://accounts.google.com/challenge".to_string());
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("VALIDATION_REQUIRED".to_string());
        let mut q = crate::models::QuotaData::new();
        q.is_forbidden = true;
        account.quota = Some(q);
        save_account(&account).unwrap();

        // 探活成功：验证挑战通过，清空验证拦截
        let res = clear_account_forbidden_and_validation(acc_id, Some("gemini-2.5-flash")).unwrap();
        assert!(res);

        let unlocked = load_account(acc_id).unwrap();
        assert!(!unlocked.validation_blocked);
        assert!(unlocked.validation_url.is_none());
        assert!(!unlocked.proxy_disabled);
        assert!(!unlocked.quota.as_ref().unwrap().is_forbidden);

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_clear_validation_blocked_accepts_lowercase_validation_reason() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-lowercase-validation-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "validation-lower@example.com");

        let mut account = load_account(acc_id).unwrap();
        account.validation_blocked = true;
        account.validation_blocked_until = Some(chrono::Utc::now().timestamp() + 600);
        account.validation_blocked_reason = Some("VALIDATION_REQUIRED: Human captcha".to_string());
        account.proxy_disabled = true;
        // 代理层写入的是小写 validation_required；旧实现区分大小写，导致永远清不掉。
        account.proxy_disabled_reason = Some("validation_required".to_string());
        let mut q = crate::models::QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some("VALIDATION_REQUIRED: Human captcha".to_string());
        account.quota = Some(q);
        save_account(&account).unwrap();

        clear_validation_blocked(&mut account, true);

        assert!(!account.validation_blocked);
        assert!(
            !account.proxy_disabled,
            "lowercase validation_required must auto-recover proxy_disabled"
        );
        assert!(account.proxy_disabled_reason.is_none());
        assert!(!account.quota.as_ref().unwrap().is_forbidden);

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_clear_validation_blocked_preserves_active_window_on_non_forced_refresh() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-active-validation-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "active-val@example.com");

        let mut account = load_account(acc_id).unwrap();
        account.validation_blocked = true;
        account.validation_blocked_until = Some(chrono::Utc::now().timestamp() + 600);
        account.validation_blocked_reason = Some("VALIDATION_REQUIRED".to_string());
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("validation_required".to_string());
        save_account(&account).unwrap();

        // 非强制调用（模拟日常配额刷新 200）
        clear_validation_blocked(&mut account, false);

        // 必须完好保留
        assert!(account.validation_blocked);
        assert!(account.validation_blocked_until.is_some());
        assert_eq!(
            account.proxy_disabled_reason.as_deref(),
            Some("validation_required")
        );

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_is_model_scoped_forbidden_reason() {
        assert!(!is_model_scoped_forbidden_reason(
            "Forbidden (403): Location not supported"
        ));
        assert!(!is_model_scoped_forbidden_reason("validation_required"));
        assert!(is_model_scoped_forbidden_reason(
            "Forbidden (403): User not authorized to access model claude-3-5-sonnet"
        ));
        assert!(is_model_scoped_forbidden_reason(
            "Forbidden (403): model gemini-3-pro is not available"
        ));
    }

    #[test]
    fn test_non_model_forbidden_lock_is_clearable_only_without_model_scope() {
        // 普通日配额 200 仍然由 update_quota / update_account_quota_internal 保持 403，
        // 见 models::account::tests::forbidden_account_keeps_quota_forbidden_after_successful_daily_refresh
        // 与 test_update_account_quota_internal_preserves_403_and_proxy_disabled。
        let token = TokenData::new(
            "test".into(),
            "test".into(),
            3600,
            None,
            None,
            None,
            false,
            None,
        );
        let mut account = Account::new("test".into(), "test@test.invalid".into(), token);
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("Forbidden (403): Location not supported".into());
        let mut q = QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some("Forbidden (403): Location not supported".into());
        account.quota = Some(q);

        assert!(has_non_model_forbidden_lock(&account));
        clear_non_model_forbidden_state(&mut account);
        assert!(!account.proxy_disabled);
        assert!(account.proxy_disabled_reason.is_none());
        assert!(!account.quota.as_ref().unwrap().is_forbidden);

        // 模型级 403：proxy 原因是通用 403，但 quota 原因明确绑定模型，必须保持锁定。
        let token = TokenData::new(
            "test".into(),
            "test".into(),
            3600,
            None,
            None,
            None,
            false,
            None,
        );
        let mut account = Account::new("test".into(), "test@test.invalid".into(), token);
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("Forbidden (403)".into());
        let mut q = QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason =
            Some("Forbidden (403): User not authorized to access model claude-3-5-sonnet".into());
        account.quota = Some(q);
        assert!(
            !has_non_model_forbidden_lock(&account),
            "任一来源带模型指向时必须保留锁"
        );

        // 纯 is_forbidden、无原因文本：按账号级 403 处理。
        let token = TokenData::new(
            "test".into(),
            "test".into(),
            3600,
            None,
            None,
            None,
            false,
            None,
        );
        let mut account = Account::new("test".into(), "test@test.invalid".into(), token);
        let mut q = QuotaData::new();
        q.is_forbidden = true;
        account.quota = Some(q);
        assert!(has_non_model_forbidden_lock(&account));

        // 验证锁不是可由数据面探针 NotBanned 清理的账号级 403。
        let token = TokenData::new(
            "test".into(),
            "test".into(),
            3600,
            None,
            None,
            None,
            false,
            None,
        );
        let mut account = Account::new("test".into(), "test@test.invalid".into(), token);
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("validation_required".into());
        assert!(!has_non_model_forbidden_lock(&account));

        // 账号级封禁优先级最高：即使存在通用 403 锁，也不允许 403 清理路径参与。
        let token = TokenData::new(
            "test".into(),
            "test".into(),
            3600,
            None,
            None,
            None,
            false,
            None,
        );
        let mut account = Account::new("test".into(), "test@test.invalid".into(), token);
        account.banned = true;
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("Forbidden (403): Location not supported".into());
        let mut q = QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some("Forbidden (403): Location not supported".into());
        account.quota = Some(q);
        assert!(
            !has_non_model_forbidden_lock(&account),
            "banned 必须压过通用 403 清理判断"
        );
    }

    #[test]
    fn test_mark_account_forbidden_does_not_downgrade_banned_account() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-banned-precedence-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "banned-precedence@example.com");

        let mut account = load_account(acc_id).unwrap();
        account.banned = true;
        account.banned_code = Some("TOS_VIOLATION".to_string());
        account.banned_reason = Some("violation of Terms of Service".to_string());
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("banned: TOS_VIOLATION".to_string());
        let mut q = crate::models::QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some("violation of Terms of Service".to_string());
        account.quota = Some(q);
        save_account(&account).unwrap();

        mark_account_forbidden(
            acc_id,
            "Forbidden (403): User not authorized to access model claude-3-5-sonnet",
        )
        .unwrap();

        let reloaded = load_account(acc_id).unwrap();
        assert!(reloaded.banned);
        assert_eq!(reloaded.banned_code.as_deref(), Some("TOS_VIOLATION"));
        assert_eq!(
            reloaded.banned_reason.as_deref(),
            Some("violation of Terms of Service")
        );
        assert!(reloaded.proxy_disabled);
        assert_eq!(
            reloaded.proxy_disabled_reason.as_deref(),
            Some("banned: TOS_VIOLATION")
        );
        assert!(reloaded.quota.as_ref().unwrap().is_forbidden);
        assert_eq!(
            reloaded.quota.as_ref().unwrap().forbidden_reason.as_deref(),
            Some("violation of Terms of Service")
        );

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_mark_account_forbidden_records_model_scope_without_disabling_account() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-model-scope-403-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "model-scope-403@example.com");

        let outcome = mark_account_forbidden(
            acc_id,
            "Forbidden (403): User not authorized to access model claude-3-5-sonnet",
        )
        .unwrap();
        assert_eq!(
            outcome,
            ForbiddenApplyResult::ModelLocked("claude-3-5-sonnet".to_string())
        );

        let reloaded = load_account(acc_id).unwrap();
        assert!(!reloaded.banned);
        assert!(!reloaded.proxy_disabled);
        assert!(reloaded.forbidden_models.contains("claude-3-5-sonnet"));
        assert!(!reloaded.quota.as_ref().is_some_and(|q| q.is_forbidden));

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_mark_account_forbidden_keeps_generic_403_account_level() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-account-scope-403-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "account-scope-403@example.com");

        let outcome =
            mark_account_forbidden(acc_id, "Forbidden (403): Location not supported").unwrap();
        assert_eq!(outcome, ForbiddenApplyResult::AccountLocked);

        let reloaded = load_account(acc_id).unwrap();
        assert!(!reloaded.banned);
        assert!(reloaded.proxy_disabled);
        assert!(reloaded.quota.as_ref().unwrap().is_forbidden);
        assert!(reloaded.forbidden_models.is_empty());

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_clear_account_forbidden_and_validation_only_removes_matching_model_scope() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-model-scope-clear-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "model-scope-clear@example.com");

        let mut account = load_account(acc_id).unwrap();
        account
            .forbidden_models
            .insert("claude-3-5-sonnet".to_string());
        account.forbidden_models.insert("gemini-3-pro".to_string());
        save_account(&account).unwrap();

        assert!(clear_account_forbidden_and_validation(acc_id, Some("claude-3-5-sonnet")).unwrap());
        let partially_cleared = load_account(acc_id).unwrap();
        assert!(!partially_cleared
            .forbidden_models
            .contains("claude-3-5-sonnet"));
        assert!(partially_cleared.forbidden_models.contains("gemini-3-pro"));

        // 再次用同一模型测活：没有匹配的 scope，不应误清另一个模型的冻结。
        assert!(
            !clear_account_forbidden_and_validation(acc_id, Some("claude-3-5-sonnet")).unwrap()
        );
        let still_gemini = load_account(acc_id).unwrap();
        assert!(still_gemini.forbidden_models.contains("gemini-3-pro"));

        assert!(clear_account_forbidden_and_validation(acc_id, Some("gemini-3-pro")).unwrap());
        assert!(load_account(acc_id).unwrap().forbidden_models.is_empty());

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_update_account_quota_internal_preserves_403_and_proxy_disabled() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-preserve-403-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "preserve403@example.com");

        let mut account = load_account(acc_id).unwrap();
        account.banned = false;
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("Forbidden (403): Location not supported".to_string());
        let mut q = crate::models::QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some("Forbidden (403): Location not supported".to_string());
        account.quota = Some(q);
        save_account(&account).unwrap();

        // 模拟日常配额刷新返回 200 OK（is_forbidden: false）
        let mut fresh_quota = crate::models::QuotaData::new();
        fresh_quota.is_forbidden = false;
        let updated = update_account_quota_internal(acc_id, fresh_quota, true).unwrap();

        assert!(
            updated.proxy_disabled,
            "日常配额刷新成功绝不能消除 403 proxy_disabled 锁定"
        );
        assert_eq!(
            updated.proxy_disabled_reason.as_deref(),
            Some("Forbidden (403): Location not supported")
        );
        assert!(
            updated.quota.as_ref().unwrap().is_forbidden,
            "日常配额刷新成功绝不能清除 403 quota.is_forbidden 状态"
        );

        // 重新从磁盘加载校验持久化一致性
        let reloaded = load_account(acc_id).unwrap();
        assert!(reloaded.proxy_disabled);
        assert!(reloaded.quota.as_ref().unwrap().is_forbidden);

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_mark_account_forbidden_with_model_without_model_in_text() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-no-model-text-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "nomodel@example.com");

        // 上游报错只有泛化 permission denied，完全没有模型名
        let generic_error = r#"{"error":{"code":403,"message":"The caller does not have permission","status":"PERMISSION_DENIED"}}"#;
        let outcome = mark_account_forbidden_with_model(
            acc_id,
            generic_error,
            Some("claude-3-5-sonnet-thinking"),
        )
        .unwrap();

        // 审查项 6：纯粹的 permission denied 不带模型语义，按账号级 Hard 403 锁定，不盲目给每个模型挂冻结
        assert_eq!(outcome, ForbiddenApplyResult::AccountLocked);

        // 当带有明确模型访问语义时，才允许通过 target_model 辅助提取为模型级冻结
        let model_error = r#"{"error":{"code":403,"message":"User not authorized to access publisher model","status":"PERMISSION_DENIED"}}"#;
        let acc_id_2 = "test-semantic-model-acc";
        create_account_file(&path_buf, acc_id_2, "semantic@example.com");
        let outcome_2 = mark_account_forbidden_with_model(
            acc_id_2,
            model_error,
            Some("claude-3-5-sonnet-thinking"),
        )
        .unwrap();

        assert_eq!(
            outcome_2,
            ForbiddenApplyResult::ModelLocked("claude-3-5-sonnet".to_string())
        );

        let reloaded_2 = load_account(acc_id_2).unwrap();
        assert!(!reloaded_2.banned);
        assert!(!reloaded_2.proxy_disabled, "单模型 403 绝不能把整号停用");
        assert!(reloaded_2.forbidden_models.contains("claude-3-5-sonnet"));
        assert!(!reloaded_2.quota.as_ref().is_some_and(|q| q.is_forbidden));

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_mark_account_forbidden_with_validation_does_not_hard_lock() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-val-no-hard-lock-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "valtest@example.com");

        let validation_error = "VALIDATION_REQUIRED: Please verify your account";
        let outcome = mark_account_forbidden(acc_id, validation_error).unwrap();

        assert_eq!(outcome, ForbiddenApplyResult::AccountLocked);

        let reloaded = load_account(acc_id).unwrap();
        assert!(!reloaded.banned);
        assert!(reloaded.validation_blocked, "必须记录验证阻断");
        assert!(reloaded.validation_blocked_until.is_some());
        assert_eq!(
            reloaded.proxy_disabled_reason.as_deref(),
            Some("validation_required")
        );
        // quota.is_forbidden 是软锁，lock_kind 必须是 Validation
        if let Some(quota) = reloaded.quota.as_ref() {
            if quota.is_forbidden {
                assert_eq!(
                    classify_lock_reason(quota.forbidden_reason.as_deref().unwrap_or("")),
                    AccountLockKind::Validation
                );
            }
        }

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_manual_proxy_disabled_preserved_across_ban_and_unban() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-manual-preserve-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "manual-pres@example.com");

        // 1. 用户手动禁用该账号
        toggle_proxy_status(acc_id, false, Some("testing manual off")).unwrap();
        let manual_off = load_account(acc_id).unwrap();
        assert!(manual_off.manual_proxy_disabled);
        assert!(manual_off.proxy_disabled);

        // 2. 账号遭遇封禁
        let ban_info = AccountBanInfo {
            code: "TOS_VIOLATION".to_string(),
            message: "Terms of Service violation".to_string(),
            appeal_url: None,
        };
        mark_account_banned(acc_id, &ban_info).unwrap();
        let banned_acc = load_account(acc_id).unwrap();
        assert!(banned_acc.banned);

        // 3. 数据面探针解封
        clear_account_ban(acc_id).unwrap();
        let unbanned_acc = load_account(acc_id).unwrap();
        assert!(!unbanned_acc.banned);
        assert!(
            unbanned_acc.manual_proxy_disabled,
            "用户手动禁用意图必须完好保留"
        );
        assert!(
            unbanned_acc.proxy_disabled,
            "系统解封绝对不能越权打开用户此前手动禁用的账号"
        );

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_clear_account_forbidden_and_validation_preserves_other_frozen_models() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-preserve-other-frozen-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "preserve-frozen@example.com");

        let mut account = load_account(acc_id).unwrap();
        account
            .forbidden_models
            .insert("claude-3-5-sonnet".to_string());
        account.forbidden_models.insert("gemini-3-pro".to_string());
        save_account(&account).unwrap();

        // 使用 gemini-3-pro 测活成功：只能解开 gemini-3-pro，claude-3-5-sonnet 必须继续冻结
        let res = clear_account_forbidden_and_validation(acc_id, Some("gemini-3-pro")).unwrap();
        assert!(res, "gemini-3-pro 模型级冻结被清理应返回 true");

        let reloaded = load_account(acc_id).unwrap();
        assert!(
            !reloaded.forbidden_models.contains("gemini-3-pro"),
            "gemini-3-pro 冻结已解除"
        );
        assert!(
            reloaded.forbidden_models.contains("claude-3-5-sonnet"),
            "未测活的 claude-3-5-sonnet 必须继续保持冻结"
        );

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_set_validation_block_does_not_set_manual_proxy_disabled() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-val-system-intent-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "val-intent@example.com");

        // 系统调用 toggle_proxy_status_with_intent，manual_intent = false
        toggle_proxy_status_with_intent(acc_id, false, Some("validation_required"), false).unwrap();

        let acc = load_account(acc_id).unwrap();
        assert!(acc.proxy_disabled, "系统锁必须标记 proxy_disabled");
        assert!(
            !acc.manual_proxy_disabled,
            "系统验证锁绝对不能被记录为用户手动禁用！"
        );
        assert_eq!(
            acc.proxy_disabled_reason.as_deref(),
            Some("validation_required")
        );

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_mark_validation_blocked_preserves_hard_403_evidence() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-preserve-hard403-on-val-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "hard403-val@example.com");

        // 构造已有 Hard 403 的账号
        let mut account = load_account(acc_id).unwrap();
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("Forbidden (403): Location not supported".to_string());
        let mut q = QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some("Forbidden (403): Location not supported".to_string());
        account.quota = Some(q);
        save_account(&account).unwrap();

        // 遭遇验证挑战
        let val_error =
            "VALIDATION_REQUIRED: verify your account at https://accounts.google.com/challenge";
        mark_account_forbidden_with_model(acc_id, val_error, None).unwrap();

        let reloaded = load_account(acc_id).unwrap();
        assert!(reloaded.validation_blocked, "必须记录验证阻断");
        assert_eq!(
            reloaded.proxy_disabled_reason.as_deref(),
            Some("Forbidden (403): Location not supported"),
            "既有的 Hard 403 proxy_disabled_reason 绝对不能被覆盖为 validation_required！"
        );
        assert_eq!(
            reloaded.quota.as_ref().unwrap().forbidden_reason.as_deref(),
            Some("Forbidden (403): Location not supported"),
            "既有的 Hard 403 quota.forbidden_reason 绝对不能被覆盖！"
        );

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[tokio::test]
    async fn test_legacy_model_forbidden_quota_migrates_on_load() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-legacy-migration-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "legacy-mig@example.com");

        // 手工写入老版本格式：模型级 403 被错误记录在 quota.is_forbidden 上
        let mut account = load_account(acc_id).unwrap();
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("Forbidden (403)".to_string());
        let mut q = QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some(
            "Forbidden (403): User not authorized to access model claude-3-5-sonnet".to_string(),
        );
        account.quota = Some(q);
        save_account(&account).unwrap();

        let manager = crate::proxy::TokenManager::new(dir.path().to_path_buf());
        let count = manager.load_accounts().await.unwrap();
        assert_eq!(
            count, 1,
            "老格式模型级 403 账号在反代加载时必须自愈并入池，绝不能被跳过！"
        );
        let token = manager.get_token_by_id(acc_id).unwrap();
        assert!(
            token.forbidden_models.contains("claude-3-5-sonnet"),
            "必须自动迁移并提取出 claude-3-5-sonnet 冻结 scope"
        );

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[tokio::test]
    async fn test_legacy_model_forbidden_preserves_manual_disabled_intent() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-legacy-manual-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "legacy-manual@example.com");

        // 手工写入老版本格式：用户手动禁用了账号，后来又挂了模型级 403
        let mut account = load_account(acc_id).unwrap();
        account.manual_proxy_disabled = true;
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some(
            "Forbidden (403): User not authorized to access model claude-3-5-sonnet".to_string(),
        );
        let mut q = QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some(
            "Forbidden (403): User not authorized to access model claude-3-5-sonnet".to_string(),
        );
        account.quota = Some(q);
        save_account(&account).unwrap();

        let manager = crate::proxy::TokenManager::new(dir.path().to_path_buf());
        let count = manager.load_accounts().await.unwrap();
        assert_eq!(
            count, 0,
            "用户手动禁用的账号即使自愈迁移了模型 403，也绝不能被自动放回调度池！"
        );

        let reloaded = load_account(acc_id).unwrap();
        assert!(reloaded.manual_proxy_disabled);
        assert!(reloaded.proxy_disabled);
        assert!(reloaded.forbidden_models.contains("claude-3-5-sonnet"));

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_generic_403_fallback_rejects_non_model_error() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-generic-403-caller-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "generic403@example.com");

        // 上游报错纯粹是 OAuth/项目级权限问题，不具备模型访问语义
        let generic_error = r#"{"error":{"code":403,"message":"The caller does not have permission","status":"PERMISSION_DENIED"}}"#;
        let outcome =
            mark_account_forbidden_with_model(acc_id, generic_error, Some("gemini-3-pro")).unwrap();

        // 必须按账号级 Hard 403 锁定，严禁盲目挂单模型冻结！
        assert_eq!(outcome, ForbiddenApplyResult::AccountLocked);

        let reloaded = load_account(acc_id).unwrap();
        assert!(
            reloaded.forbidden_models.is_empty(),
            "非模型访问语义的通用 403 绝不能挂载单模型冻结！"
        );
        assert!(reloaded.proxy_disabled, "账号级权限被拒必须对账号整体停用");
        assert!(reloaded.quota.as_ref().unwrap().is_forbidden);

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_mark_account_forbidden_with_models_records_both_target_and_alt() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-dual-model-forbidden-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "dual-model@example.com");

        // 上游报错明确指向上游动态改写后的模型 gemini-3.1-pro-high
        let upstream_error = r#"{"error":{"code":403,"message":"Access denied to models/gemini-3.1-pro-high","status":"PERMISSION_DENIED"}}"#;
        let outcome = mark_account_forbidden_with_models(
            acc_id,
            upstream_error,
            Some("gemini-3-pro"),
            Some("gemini-3.1-pro-high"),
        )
        .unwrap();

        // 主 scope 成功剥离 -high 档位后缀，消除前端卡片重复双徽章
        assert_eq!(
            outcome,
            ForbiddenApplyResult::ModelLocked("gemini-3.1-pro".to_string())
        );

        let reloaded = load_account(acc_id).unwrap();
        // 关键断言（审查项 1 & 4）：映射前模型与改写后模型全部被成功写入，且均已清洗剥离档位！
        assert!(
            reloaded.forbidden_models.contains("gemini-3-pro"),
            "映射前目标模型 gemini-3-pro 必须被写入 forbidden_models"
        );
        assert!(
            reloaded.forbidden_models.contains("gemini-3.1-pro"),
            "改写后实际上游基底模型 gemini-3.1-pro 必须被写入 forbidden_models"
        );
        assert!(
            !reloaded.forbidden_models.contains("gemini-3.1-pro-high"),
            "带 -high 档位后缀的冗余 scope 必须已被剥离，绝不向前端展示重复徽章"
        );
        assert!(!reloaded.proxy_disabled, "单模型冻结不得停用整号");

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_clear_account_forbidden_and_validation_clears_proxy_validation_lock_even_with_other_quota_hard_lock(
    ) {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-val-clear-with-quota-hard-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "val-with-hard@example.com");

        // 构造：quota 里有针对 Claude 的 Hard 403，但 proxy 停用原因属于 validation_required
        let mut account = load_account(acc_id).unwrap();
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("validation_required".to_string());
        account.validation_blocked = true;
        account.validation_blocked_until = Some(chrono::Utc::now().timestamp() + 600);
        let mut q = QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = Some(
            "Forbidden (403): User not authorized to access model claude-3-5-sonnet".to_string(),
        );
        account.quota = Some(q);
        save_account(&account).unwrap();

        // 使用 gemini 测活成功：验证挑战通过，validation 类型的 proxy 锁必须被解除！
        let res = clear_account_forbidden_and_validation(acc_id, Some("gemini-2.5-flash")).unwrap();
        assert!(res, "验证锁与关联 proxy 停用被清理应返回 true");

        let reloaded = load_account(acc_id).unwrap();
        assert!(!reloaded.validation_blocked, "验证阻断必须被清除");
        assert!(
            !reloaded.proxy_disabled,
            "属于 validation 的 proxy 锁必须被彻底解开，绝不能因异构模型 quota 403 残留为 validation_required 导致账号死锁！"
        );
        assert_eq!(reloaded.proxy_disabled_reason, None);
        assert!(
            reloaded.quota.as_ref().unwrap().is_forbidden,
            "quota 中关于 Claude 的 Hard 403 锁定依然必须受到保护"
        );

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_has_active_system_lock_rejects_empty_reason_quota_forbidden() {
        let token = TokenData::new(
            "test".into(),
            "test".into(),
            3600,
            None,
            None,
            None,
            false,
            None,
        );
        let mut account = Account::new("test".into(), "test@test.invalid".into(), token);
        let mut q = QuotaData::new();
        q.is_forbidden = true;
        q.forbidden_reason = None; // 空原因
        account.quota = Some(q);

        let now = chrono::Utc::now().timestamp();
        // 关键断言（审查项 3）：空原因或未分类的 quota.is_forbidden 必须被视为有效系统锁，严禁允许手动启用导致调度跳过！
        assert!(account.has_active_system_lock(now));
    }

    #[test]
    fn test_gemini_paired_rewrite_clear_on_single_warmup() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-gemini-paired-clear-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "gemini-pair@example.com");

        // 构造：同时冻结映射前模型与动态改写模型
        let mut account = load_account(acc_id).unwrap();
        account.forbidden_models.insert("gemini-3-pro".to_string());
        account
            .forbidden_models
            .insert("gemini-3.1-pro".to_string());
        save_account(&account).unwrap();

        // 仅对 gemini-3-pro 进行测活：成对解冻逻辑必须将关联的 gemini-3.1-pro 一同自愈解限！
        let res = clear_account_forbidden_and_validation(acc_id, Some("gemini-3-pro")).unwrap();
        assert!(res, "测活成功应返回 true");

        let reloaded = load_account(acc_id).unwrap();
        assert!(
            reloaded.forbidden_models.is_empty(),
            "Gemini 关联改写模型必须成对解冻，一次测活彻底清空"
        );

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_claude_models_remain_strictly_isolated() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-claude-isolation-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "claude-iso@example.com");

        // 构造：同时冻结 Sonnet 与 Opus
        let mut account = load_account(acc_id).unwrap();
        account
            .forbidden_models
            .insert("claude-3-5-sonnet".to_string());
        account.forbidden_models.insert("claude-3-opus".to_string());
        save_account(&account).unwrap();

        // 仅对 Sonnet 测活：Sonnet 必须被解开，但 Opus 绝不能被过度合并解封！
        let res =
            clear_account_forbidden_and_validation(acc_id, Some("claude-3-5-sonnet")).unwrap();
        assert!(res, "测活 Sonnet 成功");

        let reloaded = load_account(acc_id).unwrap();
        assert!(
            !reloaded.forbidden_models.contains("claude-3-5-sonnet"),
            "Sonnet 冻结已解除"
        );
        assert!(
            reloaded.forbidden_models.contains("claude-3-opus"),
            "Opus 绝不能被 Sonnet 测活误解限，必须继续保持隔离冻结！"
        );

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_set_account_index_proxy_disabled_with_lock() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-index-helper-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "index-helper@example.com");

        // 初始化索引
        let mut index = AccountIndex::new();
        index.accounts.push(AccountSummary {
            id: acc_id.to_string(),
            email: "index-helper@example.com".to_string(),
            name: None,
            disabled: false,
            proxy_disabled: true,
            protected_models: std::collections::HashSet::new(),
            created_at: 0,
            last_used: 0,
        });
        save_account_index(&index).unwrap();

        // 调用 helper 将 proxy_disabled 置为 false
        set_account_index_proxy_disabled(acc_id, false).unwrap();

        let loaded = load_account_index().unwrap();
        assert!(
            !loaded
                .accounts
                .iter()
                .find(|a| a.id == acc_id)
                .unwrap()
                .proxy_disabled,
            "全局锁 helper 必须正确更新索引 proxy_disabled 状态"
        );

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_gemini_pro_and_flash_do_not_clear_each_other() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-gemini-pro-flash-iso-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "pro-flash-iso@example.com");

        // 场景 A：冻结 gemini-3-pro，测活高阶 Flash gemini-3.8-flash 成功 -> Pro 冻结坚决保留！
        let mut account = load_account(acc_id).unwrap();
        account.forbidden_models.insert("gemini-3-pro".to_string());
        save_account(&account).unwrap();

        let res = clear_account_forbidden_and_validation(acc_id, Some("gemini-3.8-flash")).unwrap();
        assert!(!res, "测活 Flash 不得清除 Pro 冻结");
        let reloaded = load_account(acc_id).unwrap();
        assert!(
            reloaded.forbidden_models.contains("gemini-3-pro"),
            "Flash 测活绝对不能误解封 Pro 冻结！"
        );

        // 场景 B：冻结 gemini-3.8-flash，测活 gemini-3-pro 成功 -> Flash 冻结坚决保留！
        let mut account = load_account(acc_id).unwrap();
        account.forbidden_models.clear();
        account
            .forbidden_models
            .insert("gemini-3.8-flash".to_string());
        save_account(&account).unwrap();

        let res = clear_account_forbidden_and_validation(acc_id, Some("gemini-3-pro")).unwrap();
        assert!(!res, "测活 Pro 不得清除 Flash 冻结");
        let reloaded = load_account(acc_id).unwrap();
        assert!(
            reloaded.forbidden_models.contains("gemini-3.8-flash"),
            "Pro 测活绝对不能误解封 Flash 冻结！"
        );

        std::env::remove_var("ABV_DATA_DIR");
    }

    #[test]
    fn test_gemini_basic_flash_does_not_clear_across_generations() {
        let _guard = SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = TestDataDir::new();
        std::env::set_var("ABV_DATA_DIR", dir.path());

        let acc_id = "test-flash-generations-acc";
        let path_buf = dir.path().clone();
        create_account_file(&path_buf, acc_id, "flash-gen@example.com");

        // 场景：冻结 gemini-2.5-flash，测活 gemini-3.0-flash 成功 -> 2.5-flash 坚决保留！
        let mut account = load_account(acc_id).unwrap();
        account
            .forbidden_models
            .insert("gemini-2.5-flash".to_string());
        save_account(&account).unwrap();

        let res = clear_account_forbidden_and_validation(acc_id, Some("gemini-3.0-flash")).unwrap();
        assert!(!res, "基础 Flash 跨代不得成对清除");
        let reloaded = load_account(acc_id).unwrap();
        assert!(
            reloaded.forbidden_models.contains("gemini-2.5-flash"),
            "gemini-2.5-flash 冻结必须完好保留！"
        );

        std::env::remove_var("ABV_DATA_DIR");
    }
}

/// Global account write lock to prevent corruption during concurrent operations
static ACCOUNT_INDEX_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub(crate) fn lock_account_file_updates() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))
}

// ... existing constants ...
#[allow(dead_code)]
const DATA_DIR: &str = ".antigravity_tools";
#[allow(dead_code)]
const LOCATION_POINTER_FILE: &str = ".antigravity_tools_location";
const ACCOUNTS_INDEX: &str = "accounts.json";
const ACCOUNTS_DIR: &str = "accounts";
const DATA_DIR_POINTER_FILE: &str = "data_dir.txt";

/// 获取数据目录自举指针文件路径（保存在系统标准配置目录下）
pub fn get_data_dir_pointer_file() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("antigravity-tools").join(DATA_DIR_POINTER_FILE))
}

static DATA_DIR_OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

fn data_dir_override_slot() -> &'static RwLock<Option<PathBuf>> {
    DATA_DIR_OVERRIDE.get_or_init(|| RwLock::new(None))
}

/// 数据目录指针文件的路径。
///
/// 可用 `ABV_DATA_DIR_POINTER_FILE` 覆盖（测试 / Docker 用）。
///
/// 为什么必须支持覆盖：单元测试会调用 `migrate_data_dir`，它经由 `apply_data_dir`
/// 写入这个指针。若指针固定指向真实的 `~/.antigravity_tools_location`，那么测试一旦
/// 被中断（Ctrl-C、超时、崩溃、进程被杀），恢复逻辑就不会执行，指针会被永久留在
/// 临时目录上 —— 应用下次启动就会读到一个空的数据目录，表现为「账号全部消失」。
fn location_pointer_path() -> Result<PathBuf, String> {
    if let Ok(custom) = std::env::var("ABV_DATA_DIR_POINTER_FILE") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return Ok(normalize_data_dir_path(trimmed));
        }
    }
    #[cfg(test)]
    {
        return Err(
            "location pointer reading disabled in tests without explicit ABV_DATA_DIR_POINTER_FILE"
                .to_string(),
        );
    }
    #[cfg(not(test))]
    {
        let home = dirs::home_dir().ok_or("failed_to_get_home_dir")?;
        Ok(home.join(LOCATION_POINTER_FILE))
    }
}

fn default_data_dir() -> Result<PathBuf, String> {
    #[cfg(test)]
    {
        return Err("default_data_dir disabled in tests to protect host".to_string());
    }
    #[cfg(not(test))]
    {
        let home = dirs::home_dir().ok_or("failed_to_get_home_dir")?;
        Ok(home.join(DATA_DIR))
    }
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    if !path.exists() {
        fs::create_dir_all(path).map_err(|e| format!("failed_to_create_data_dir: {}", e))?;
    }
    Ok(())
}

/// Strip Windows `\\?\` / `\\?\UNC\` prefixes and quotes so paths stay portable
/// across Windows, Linux, macOS and Docker (`ABV_DATA_DIR=/app/data`).
fn strip_extended_path_prefix(input: &str) -> String {
    let s = input
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '\u{feff}');
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{}", rest);
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return rest.to_string();
    }
    if let Some(rest) = s.strip_prefix("//?/UNC/") {
        return format!("//{}", rest);
    }
    if let Some(rest) = s.strip_prefix("//?/") {
        return rest.to_string();
    }
    s.to_string()
}

fn expand_user_path(input: &str) -> Option<PathBuf> {
    if input == "~" || input.starts_with("~/") || input.starts_with("~\\") {
        let home = dirs::home_dir()?;
        let rest = input
            .trim_start_matches('~')
            .trim_start_matches(['/', '\\']);
        return Some(if rest.is_empty() {
            home
        } else {
            home.join(rest)
        });
    }
    None
}

/// Normalize a data-dir path for persistence, env vars and UI display.
pub fn normalize_data_dir_path(path: impl AsRef<Path>) -> PathBuf {
    let raw = path.as_ref().to_string_lossy();
    let stripped = strip_extended_path_prefix(&raw);
    if let Some(expanded) = expand_user_path(&stripped) {
        return expanded;
    }
    PathBuf::from(stripped)
}

/// Human-readable path without Windows verbatim prefixes.
pub fn format_data_dir_path(path: &Path) -> String {
    normalize_data_dir_path(path).to_string_lossy().into_owned()
}

fn resolve_existing_path(path: &Path) -> PathBuf {
    let normalized = normalize_data_dir_path(path);
    match normalized.canonicalize() {
        Ok(canon) => normalize_data_dir_path(canon),
        Err(_) => normalized,
    }
}

fn path_compare_key(path: &Path) -> String {
    let mut s = resolve_existing_path(path)
        .to_string_lossy()
        .replace('\\', "/");
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    #[cfg(windows)]
    {
        s = s.to_ascii_lowercase();
    }
    s
}

fn paths_equivalent(a: &Path, b: &Path) -> bool {
    path_compare_key(a) == path_compare_key(b)
}

fn is_nested_data_dir(inner: &Path, outer: &Path) -> bool {
    let inner_key = path_compare_key(inner);
    let outer_key = path_compare_key(outer);
    inner_key != outer_key && inner_key.starts_with(&(outer_key + "/"))
}

fn persist_clean_env(dir: &Path) {
    std::env::set_var("ABV_DATA_DIR", format_data_dir_path(dir));
}

fn read_location_pointer() -> Option<PathBuf> {
    let path = location_pointer_path().ok()?;
    let content = fs::read_to_string(path).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let cleaned = normalize_data_dir_path(trimmed);
    if format_data_dir_path(&cleaned) != trimmed {
        let _ = write_location_pointer(&cleaned);
    }
    Some(cleaned)
}

fn write_location_pointer(dir: &Path) -> Result<(), String> {
    let pointer = location_pointer_path()?;
    fs::write(&pointer, format_data_dir_path(dir).as_bytes())
        .map_err(|e| format!("写入数据目录指针失败: {}", e))
}

fn is_default_data_dir(dir: &Path) -> bool {
    default_data_dir()
        .map(|d| paths_equivalent(&d, dir) || d == dir)
        .unwrap_or(false)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("创建目标数据目录失败: {}", e))?;
    for entry in fs::read_dir(src).map_err(|e| format!("读取原数据目录失败: {}", e))? {
        let entry = entry.map_err(|e| format!("读取数据目录项失败: {}", e))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|e| format!("读取数据目录项类型失败: {}", e))?;
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("创建目标子目录失败: {}", e))?;
            }
            fs::copy(&from, &to).map_err(|e| format!("复制文件失败 {}: {}", from.display(), e))?;
        }
    }
    Ok(())
}

fn dir_is_empty(path: &Path) -> Result<bool, String> {
    let mut entries = fs::read_dir(path).map_err(|e| format!("读取目标目录失败: {}", e))?;
    Ok(entries.next().is_none())
}

fn apply_data_dir(dir: &Path) -> Result<(), String> {
    let dir = normalize_data_dir_path(dir);
    ensure_dir(&dir)?;
    if is_default_data_dir(&dir) {
        if let Ok(pointer) = location_pointer_path() {
            let _ = fs::remove_file(pointer);
        }
    } else {
        write_location_pointer(&dir)?;
    }
    if let Ok(mut guard) = data_dir_override_slot().write() {
        *guard = Some(dir.clone());
    }
    persist_clean_env(&dir);
    Ok(())
}

/// Get data directory path
pub fn get_data_dir() -> Result<PathBuf, String> {
    // 1. Process env (tests, Docker, and in-process override after migrate)
    if let Ok(env_path) = std::env::var("ABV_DATA_DIR") {
        if !env_path.trim().is_empty() {
            let data_dir = normalize_data_dir_path(&env_path);
            ensure_dir(&data_dir)?;
            if format_data_dir_path(&data_dir) != env_path {
                persist_clean_env(&data_dir);
            }
            return Ok(data_dir);
        }
    }

    // 2. Runtime override (pointer already loaded this session)
    if let Ok(guard) = data_dir_override_slot().read() {
        if let Some(ref path) = *guard {
            let data_dir = normalize_data_dir_path(path);
            ensure_dir(&data_dir)?;
            return Ok(data_dir);
        }
    }

    // 3. Pointer file outside the data dir so deleting the old folder still finds the new path
    if let Some(path) = read_location_pointer() {
        ensure_dir(&path)?;
        if let Ok(mut guard) = data_dir_override_slot().write() {
            *guard = Some(path.clone());
        }
        return Ok(path);
    }

    // 4. Default ~/.antigravity_tools
    #[cfg(test)]
    {
        // [DEFENSIVE] 在单元测试执行期间，严禁回落并写入真实宿主机的 ~/.antigravity_tools 目录！
        // 若测试未显式设置 ABV_DATA_DIR，为整个测试进程分配唯一的临时沙盒目录，
        // 保证跨多线程（如 thread::spawn）与多步操作（建表->读写）指向同一个沙盒，
        // 且绝对不污染宿主机真实的 ~/.antigravity_tools 目录
        static TEST_FALLBACK_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        let fallback_test_dir = TEST_FALLBACK_DIR
            .get_or_init(|| {
                std::env::temp_dir().join(format!(
                    "antigravity_unit_test_fallback_sandbox_{}",
                    std::process::id()
                ))
            })
            .clone();
        ensure_dir(&fallback_test_dir)?;
        Ok(fallback_test_dir)
    }
    #[cfg(not(test))]
    {
        let data_dir = default_data_dir()?;
        ensure_dir(&data_dir)?;
        Ok(data_dir)
    }
}

/// Move the data directory to `new_dir`, persist the location, and switch all runtime lookups.
pub fn migrate_data_dir(new_dir: PathBuf) -> Result<PathBuf, String> {
    let new_dir = normalize_data_dir_path(new_dir);
    let new_dir = if new_dir.as_os_str().is_empty() {
        return Err("目标数据目录不能为空".to_string());
    } else if new_dir.is_absolute() {
        new_dir
    } else {
        std::env::current_dir()
            .map_err(|e| format!("无法解析相对路径: {}", e))?
            .join(new_dir)
    };

    let old_dir = normalize_data_dir_path(get_data_dir()?);
    if paths_equivalent(&old_dir, &new_dir) {
        apply_data_dir(&old_dir)?;
        return Ok(resolve_existing_path(&old_dir));
    }

    if is_nested_data_dir(&new_dir, &old_dir) {
        return Err("不能把数据目录迁移到自身内部".to_string());
    }

    if new_dir.exists() {
        if new_dir.is_file() {
            return Err("目标路径已存在且不是目录".to_string());
        }
        if !dir_is_empty(&new_dir)? {
            return Err("目标目录不是空文件夹，请选择空目录或新路径".to_string());
        }
        copy_dir_recursive(&old_dir, &new_dir)?;
        let _ = fs::remove_dir_all(&old_dir);
    } else if let Some(parent) = new_dir.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目标父目录失败: {}", e))?;
        match fs::rename(&old_dir, &new_dir) {
            Ok(()) => {}
            Err(_) => {
                copy_dir_recursive(&old_dir, &new_dir)?;
                let _ = fs::remove_dir_all(&old_dir);
            }
        }
    } else {
        return Err("目标路径无效".to_string());
    }

    let resolved = resolve_existing_path(&new_dir);
    apply_data_dir(&resolved)?;
    Ok(resolved)
}

/// Get accounts directory path
pub fn get_accounts_dir() -> Result<PathBuf, String> {
    let data_dir = get_data_dir()?;
    let accounts_dir = data_dir.join(ACCOUNTS_DIR);

    if !accounts_dir.exists() {
        fs::create_dir_all(&accounts_dir)
            .map_err(|e| format!("failed_to_create_accounts_dir: {}", e))?;
    }

    Ok(accounts_dir)
}

/// Load account index from a specific directory (internal helper)
const ACCOUNTS_BACKUP_INDEX: &str = "accounts.json-backup";

/// [NEW #3726] 尝试从旧版单文件凭据结构中无缝迁移并生成新版分片文件
/// 1. 尝试将内容解析为旧版单文件全量账号（Vec<Account> 或包含 accounts 的对象）；
/// 2. 成功解析后，为旧版单文件创建 `<filename>-backup` 备份（如 accounts.json-backup）；
/// 3. 为每个账号无缝分片生成新版 accounts/{uuid}.json 文件；
/// 4. 构造全新的轻量级 AccountIndex 并写盘；
/// 5. 过程中若出现失败，原文件和备份文件保持完好，依然可从中随时重新导入。
fn try_migrate_legacy_single_file(data_dir: &PathBuf, raw_content: &str) -> Option<AccountIndex> {
    let mut parsed_accounts: Vec<Account> = serde_json::from_str::<Vec<Account>>(raw_content)
        .ok()
        .unwrap_or_default();

    if parsed_accounts.is_empty() {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw_content) {
            if let Some(arr) = val.get("accounts").and_then(|v| v.as_array()) {
                for item in arr {
                    if let Ok(acc) = serde_json::from_value::<Account>(item.clone()) {
                        parsed_accounts.push(acc);
                    }
                }
            } else if let Some(obj) = val.as_object() {
                // V1 格式字典映射: { "uuid": { "email": ..., "token": ... } }
                for (_k, v) in obj {
                    if let Ok(acc) = serde_json::from_value::<Account>(v.clone()) {
                        parsed_accounts.push(acc);
                    }
                }
            }
        }
    }

    if parsed_accounts.is_empty() {
        return None;
    }

    crate::modules::logger::log_info(&format!(
        "🔄 [Legacy Migration] Discovered legacy single accounts file with {} accounts. Upgrading to segmented architecture...",
        parsed_accounts.len()
    ));

    // 1. 创建 accounts.json-backup 备份
    let backup_path = data_dir.join(ACCOUNTS_BACKUP_INDEX);
    if let Err(e) = crate::utils::fs::write_atomic(&backup_path, raw_content.as_bytes()) {
        crate::modules::logger::log_warn(&format!(
            "Failed to create legacy backup at {:?}: {}",
            backup_path, e
        ));
    } else {
        crate::modules::logger::log_info(&format!(
            "📦 [Legacy Migration] Preserved original legacy file as {:?}",
            backup_path
        ));
    }

    // 2. 为每个账号无缝生成 accounts/{uuid}.json
    let accounts_dir = data_dir.join(ACCOUNTS_DIR);
    let _ = fs::create_dir_all(&accounts_dir);

    let mut summaries = Vec::new();
    for acc in &parsed_accounts {
        let acc_path = accounts_dir.join(format!("{}.json", acc.id));
        if let Ok(content) = serde_json::to_string_pretty(acc) {
            let _ = crate::utils::fs::write_atomic(&acc_path, content.as_bytes());
        }
        summaries.push(AccountSummary {
            id: acc.id.clone(),
            email: acc.email.clone(),
            name: acc.name.clone(),
            disabled: acc.disabled,
            proxy_disabled: acc.proxy_disabled,
            protected_models: acc.protected_models.clone(),
            created_at: acc.created_at,
            last_used: acc.last_used,
        });
    }

    let first_id = summaries.first().map(|s| s.id.clone());
    let new_index = AccountIndex {
        version: "2.0".to_string(),
        accounts: summaries,
        current_account_id: first_id,
        current_target_ide: None,
        current_classic_account_id: None,
        current_ide_account_id: None,
        current_agy_account_id: None,
        current_acp_account_id: None,
    };

    // 3. 写入新版规范索引
    if let Err(e) = save_account_index_in_dir(data_dir, &new_index) {
        crate::modules::logger::log_error(&format!("Failed to save migrated index: {}", e));
        return None;
    }

    crate::modules::logger::log_info(
        "🎉 [Legacy Migration] Upgraded successfully! All accounts are segmented and index generated.",
    );

    Some(new_index)
}

fn load_account_index_in_dir(data_dir: &PathBuf) -> Result<AccountIndex, String> {
    let index_path = data_dir.join(ACCOUNTS_INDEX);

    if !index_path.exists() {
        crate::modules::logger::log_warn(
            "Account index file not found, checking backup or accounts directory",
        );
        // [FIX #3726] 优先检查是否存在 accounts.json-backup 备份文件，支持失败无缝重新导入
        let backup_path = data_dir.join(ACCOUNTS_BACKUP_INDEX);
        if backup_path.exists() {
            if let Ok(backup_content) = fs::read_to_string(&backup_path) {
                if let Some(restored) = try_migrate_legacy_single_file(data_dir, &backup_content) {
                    crate::modules::logger::log_info(
                        "✅ [Recovery] Successfully restored accounts from accounts.json-backup!",
                    );
                    return Ok(restored);
                }
            }
        }

        let recovered = rebuild_index_from_accounts_in_dir(data_dir)?;
        try_save_recovered_index(data_dir, &index_path, &recovered, None)?;
        return Ok(recovered);
    }

    let raw_content =
        fs::read(&index_path).map_err(|e| format!("failed_to_read_account_index: {}", e))?;

    // If file is empty, attempt recovery
    if raw_content.is_empty() {
        crate::modules::logger::log_warn(
            "Account index is empty, checking backup or accounts directory",
        );
        let backup_path = data_dir.join(ACCOUNTS_BACKUP_INDEX);
        if backup_path.exists() {
            if let Ok(backup_content) = fs::read_to_string(&backup_path) {
                if let Some(restored) = try_migrate_legacy_single_file(data_dir, &backup_content) {
                    return Ok(restored);
                }
            }
        }
        let recovered = rebuild_index_from_accounts_in_dir(data_dir)?;
        try_save_recovered_index(data_dir, &index_path, &recovered, None)?;
        return Ok(recovered);
    }

    // Sanitize content: strip BOM and leading NUL bytes
    let sanitized = sanitize_index_content(&raw_content);

    // If sanitized content is empty/whitespace, attempt recovery
    if sanitized.trim().is_empty() {
        crate::modules::logger::log_warn(
            "Account index is empty after sanitization, checking backup or accounts directory",
        );
        let backup_path = data_dir.join(ACCOUNTS_BACKUP_INDEX);
        if backup_path.exists() {
            if let Ok(backup_content) = fs::read_to_string(&backup_path) {
                if let Some(restored) = try_migrate_legacy_single_file(data_dir, &backup_content) {
                    return Ok(restored);
                }
            }
        }
        let recovered = rebuild_index_from_accounts_in_dir(data_dir)?;
        try_save_recovered_index(data_dir, &index_path, &recovered, None)?;
        return Ok(recovered);
    }

    // 1. 优先尝试作为新版规范的轻量索引 (AccountIndex) 解析（毫秒级瞬时直出）
    if let Ok(index) = serde_json::from_str::<AccountIndex>(&sanitized) {
        crate::modules::logger::log_info(&format!(
            "Successfully loaded index with {} accounts",
            index.accounts.len()
        ));
        return Ok(index);
    }

    // 2. 若新版格式未命中，才检查是否为旧版单文件格式，无缝触发自动分片并创建 -backup
    if let Some(migrated) = try_migrate_legacy_single_file(data_dir, &sanitized) {
        return Ok(migrated);
    }

    // 3. 仍无法解析时，执行容灾重建
    crate::modules::logger::log_error(
        "Failed to parse account index as new or legacy format. Attempting recovery from accounts directory",
    );
    let recovered = rebuild_index_from_accounts_in_dir(data_dir)?;
    try_save_recovered_index(data_dir, &index_path, &recovered, Some(&raw_content))?;
    Ok(recovered)
}

/// Save account index to a specific directory (internal helper)
fn save_account_index_in_dir(data_dir: &PathBuf, index: &AccountIndex) -> Result<(), String> {
    let index_path = data_dir.join(ACCOUNTS_INDEX);

    let content = serde_json::to_string_pretty(index)
        .map_err(|e| format!("failed_to_serialize_account_index: {}", e))?;

    crate::utils::fs::write_atomic(&index_path, content.as_bytes())
        .map_err(|e| format!("failed_to_save_account_index: {}", e))
}

/// Rebuild AccountIndex by scanning accounts/*.json files in specific directory
fn rebuild_index_from_accounts_in_dir(data_dir: &PathBuf) -> Result<AccountIndex, String> {
    let accounts_dir = data_dir.join(ACCOUNTS_DIR);
    let mut summaries = Vec::new();

    if accounts_dir.exists() {
        if let Ok(entries) = fs::read_dir(&accounts_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "json") {
                    if let Some(account_id) = path.file_stem().and_then(|s| s.to_str()) {
                        match load_account_at_path(&path) {
                            Ok(account) => {
                                summaries.push(AccountSummary {
                                    id: account.id,
                                    email: account.email,
                                    name: account.name,
                                    disabled: account.disabled,
                                    proxy_disabled: account.proxy_disabled,
                                    protected_models: account.protected_models,
                                    created_at: account.created_at,
                                    last_used: account.last_used,
                                });
                            }
                            Err(e) => {
                                crate::modules::logger::log_warn(&format!(
                                    "Failed to load account {} during recovery: {}",
                                    account_id, e
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    // Sort by last_used desc, then by email for deterministic order
    summaries.sort_by(|a, b| {
        b.last_used
            .cmp(&a.last_used)
            .then_with(|| a.email.cmp(&b.email))
    });

    let current_account_id = summaries.first().map(|s| s.id.clone());

    crate::modules::logger::log_info(&format!(
        "Rebuilt index from accounts directory: {} accounts recovered",
        summaries.len()
    ));

    Ok(AccountIndex {
        version: "2.0".to_string(),
        accounts: summaries,
        current_account_id,
        current_target_ide: None,
        current_classic_account_id: None,
        current_ide_account_id: None,
        current_agy_account_id: None,
        current_acp_account_id: None,
    })
}

/// Load account from a specific path with self-healing support for trailing characters/corrupted suffixes
fn load_account_at_path(account_path: &PathBuf) -> Result<Account, String> {
    let content = fs::read_to_string(account_path)
        .map_err(|e| format!("failed_to_read_account_data: {}", e))?;

    let mut account = match serde_json::from_str::<Account>(&content) {
        Ok(account) => account,
        Err(e) => {
            let err_msg = e.to_string();
            // Self-healing attempt: handle trailing characters / extra closing brackets
            if err_msg.contains("trailing characters")
                || err_msg.contains("trailing comma")
                || err_msg.contains("trailing")
            {
                let mut de = serde_json::Deserializer::from_str(&content);
                if let Ok(account) = serde::Deserialize::deserialize(&mut de) {
                    crate::modules::logger::log_warn(&format!(
                        "Self-healing account JSON at {:?}: recovered valid account data from trailing characters, saving clean file",
                        account_path
                    ));
                    let _ = save_account_at_path(account_path, &account);
                    return Ok(account);
                }
            }
            return Err(format!("failed_to_parse_account_data: {}", err_msg));
        }
    };

    // Self-healing: if subscription_tier is missing or unnormalized, heal it and persist to disk
    if let Some(ref mut quota) = account.quota {
        let original_tier = quota.subscription_tier.clone();
        quota.ensure_subscription_tier();
        if quota.subscription_tier != original_tier {
            crate::modules::logger::log_info(&format!(
                "Self-healing subscription tier for account {} ({:?} -> {:?})",
                account.email, original_tier, quota.subscription_tier
            ));
            let _ = save_account_at_path(account_path, &account);
        }
    }

    Ok(account)
}

/// Load account index with recovery support
pub fn load_account_index() -> Result<AccountIndex, String> {
    let data_dir = get_data_dir()?;
    load_account_index_in_dir(&data_dir)
}

/// 轻量读取账号索引中的账号数（供启动阶段轮询，不触发恢复流程与日志放大）
pub fn indexed_account_count() -> usize {
    let Ok(data_dir) = get_data_dir() else {
        return 0;
    };
    let index_path = data_dir.join(ACCOUNTS_INDEX);
    let Ok(raw) = fs::read(&index_path) else {
        return 0;
    };
    let sanitized = sanitize_index_content(&raw);
    serde_json::from_str::<AccountIndex>(&sanitized)
        .map(|index| index.accounts.len())
        .unwrap_or(0)
}

/// Sanitize index file content by stripping BOM and leading NUL bytes
fn sanitize_index_content(raw: &[u8]) -> String {
    // Skip UTF-8 BOM if present
    let without_bom = if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &raw[3..]
    } else {
        raw
    };

    // Skip leading NUL bytes
    let without_nul = without_bom
        .iter()
        .skip_while(|&&b| b == 0x00)
        .copied()
        .collect::<Vec<u8>>();

    // Convert to string (lossy - invalid UTF-8 sequences become replacement chars)
    String::from_utf8_lossy(&without_nul).into_owned()
}

/// Best-effort save of recovered index without deadlocking
fn try_save_recovered_index(
    data_dir: &PathBuf,
    _index_path: &PathBuf,
    index: &AccountIndex,
    corrupt_content: Option<&[u8]>,
) -> Result<(), String> {
    // Backup corrupt file if content provided
    if let Some(content) = corrupt_content {
        let timestamp = chrono::Utc::now().timestamp();
        let backup_name = format!("accounts.json.corrupt-{}-{}", timestamp, Uuid::new_v4());
        let backup_path = data_dir.join(&backup_name);
        if let Err(e) = crate::utils::fs::write_atomic(&backup_path, content) {
            crate::modules::logger::log_warn(&format!(
                "Failed to backup corrupt index to {}: {}",
                backup_name, e
            ));
        } else {
            crate::modules::logger::log_info(&format!(
                "Backed up corrupt index to {}",
                backup_name
            ));
        }
    }

    // Try to acquire lock without blocking - if we can't get it, skip saving
    match ACCOUNT_INDEX_LOCK.try_lock() {
        Ok(_guard) => {
            if let Err(e) = save_account_index_in_dir(data_dir, index) {
                crate::modules::logger::log_warn(&format!(
                    "Failed to save recovered index: {}. Will retry on next load.",
                    e
                ));
            } else {
                crate::modules::logger::log_info("Successfully saved recovered index");
            }
        }
        Err(_) => {
            crate::modules::logger::log_warn(
                "Could not acquire lock to save recovered index. Will retry on next load.",
            );
        }
    }

    Ok(())
}

/// Save account index (atomic write)
pub fn save_account_index(index: &AccountIndex) -> Result<(), String> {
    let data_dir = get_data_dir()?;
    save_account_index_in_dir(&data_dir, index)
}

/// Load account data
pub fn load_account(account_id: &str) -> Result<Account, String> {
    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account_id));
    load_account_at_path(&account_path)
}

/// Save account data at specific file path (thread-safe and atomic)
fn save_account_at_path(account_path: &PathBuf, account: &Account) -> Result<(), String> {
    let _lock = get_account_lock(&account.id);
    let _guard = _lock.lock().unwrap();

    let content = serde_json::to_string_pretty(account)
        .map_err(|e| format!("failed_to_serialize_account_data: {}", e))?;

    crate::utils::fs::write_atomic(account_path, content.as_bytes())
        .map_err(|e| format!("failed_to_save_account_file: {}", e))
}

/// [NEW #3726] 确保同邮箱账号在物理磁盘上永远只有一个凭据文件（严格幂等防重）
/// 扫描 accounts/ 目录，若发现其他物理文件的 email 与当前账号相同但 ID 不同，主动清理历史残留文件
fn clean_duplicate_account_files(accounts_dir: &Path, active_id: &str, email: &str) {
    let norm_email = email.trim().to_lowercase();
    if norm_email.is_empty() || !accounts_dir.exists() {
        return;
    }

    if let Ok(entries) = fs::read_dir(accounts_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if stem != active_id {
                        if let Ok(content) = fs::read_to_string(&path) {
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                                if let Some(e) = val.get("email").and_then(|v| v.as_str()) {
                                    if e.trim().to_lowercase() == norm_email {
                                        crate::modules::logger::log_warn(&format!(
                                            "🧹 [Idempotency] Cleaning up duplicate orphan account file {:?} for email {}",
                                            path, email
                                        ));
                                        let _ = fs::remove_file(&path);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Save account data (thread-safe and atomic)
pub fn save_account(account: &Account) -> Result<(), String> {
    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account.id));
    save_account_at_path(&account_path, account)?;
    clean_duplicate_account_files(&accounts_dir, &account.id, &account.email);
    Ok(())
}

/// Save proxy selection priority using the shared account write lock.
pub fn update_account_priority(account_id: &str, priority: u8) -> Result<(), String> {
    update_account_priority_with_models(account_id, priority, None).map(|_| ())
}

/// Save proxy selection priority and optional per-model-family overrides ("gemini" / "claude").
pub fn update_account_priority_with_models(
    account_id: &str,
    priority: u8,
    model_priorities: Option<std::collections::HashMap<String, u8>>,
) -> Result<std::collections::HashMap<String, u8>, String> {
    crate::models::account::validate_priority(priority)?;
    let validated_model_priorities = match model_priorities {
        Some(ref mp) => Some(crate::models::account::validate_model_priorities(mp)?),
        None => None,
    };
    let _account_write = lock_account_file_updates()?;
    let mut account = load_account(account_id)?;
    account.priority = priority;
    if let Some(mp) = validated_model_priorities {
        account.model_priorities = mp;
    }
    save_account(&account)?;
    Ok(account.model_priorities)
}

/// Update account GCP Project ID.
///
/// When project_id is provided, sets is_gcp_tos to true so the gateway
/// seamlessly routes requests to the enterprise/Vertex AI platform.
pub fn update_account_project_id(
    account_id: &str,
    project_id: Option<String>,
) -> Result<(), String> {
    let _account_write = lock_account_file_updates()?;
    let mut account = load_account(account_id)?;
    let cleaned_pid = project_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if cleaned_pid.is_some() {
        account.token.is_gcp_tos = true;
    }
    account.token.project_id = cleaned_pid;
    save_account(&account)
}

/// Automatically fetch and associate Project ID from upstream for a given account.
pub async fn auto_fetch_account_project_id(account_id: &str) -> Result<String, String> {
    let mut account = load_account(account_id)?;

    // Ensure access token is valid before querying upstream
    let token = crate::modules::oauth::ensure_fresh_token(&account.token, Some(&account.id))
        .await
        .map_err(|e| format!("Failed to refresh token: {}", e))?;

    if token.access_token != account.token.access_token {
        account.token = token.clone();
        let _ = save_account(&account);
    }

    match crate::proxy::project_resolver::fetch_project_id_for_account(
        &token.access_token,
        Some(account_id),
    )
    .await
    {
        Ok(pid) => {
            update_account_project_id(account_id, Some(pid.clone()))?;
            Ok(pid)
        }
        Err(err) => Err(err),
    }
}

/// List all accounts
pub fn list_accounts() -> Result<Vec<Account>, String> {
    crate::modules::logger::log_info("Listing accounts...");
    let mut index = load_account_index()?;
    let mut accounts = Vec::new();
    let mut index_dirty = false;

    for summary in &mut index.accounts {
        match load_account(&summary.id) {
            Ok(account) => {
                // 实体文件是账号状态的权威来源。若索引中的停用状态与实体文件发生漂移，以实体文件为准并同步修正索引
                if summary.disabled != account.disabled {
                    summary.disabled = account.disabled;
                    index_dirty = true;
                }
                if summary.proxy_disabled != account.proxy_disabled {
                    summary.proxy_disabled = account.proxy_disabled;
                    index_dirty = true;
                }
                accounts.push(account);
            }
            Err(e) => {
                crate::modules::logger::log_error(&format!(
                    "Failed to load account {}: {}",
                    summary.id, e
                ));
                // [FIX #929] Removed auto-repair logic.
                // We no longer silently delete account IDs from the index if the file is missing.
                // This prevents account loss during version upgrades or temporary FS issues.
            }
        }
    }

    if index_dirty {
        let _ = save_account_index(&index);
    }

    if let Err(error) = crate::modules::token_stats::populate_weekly_usage(&mut accounts) {
        tracing::warn!("Weekly token usage unavailable: {}", error);
    }
    Ok(accounts)
}

/// Resolve persisted CLI credentials without changing any client's login state.
pub fn find_agy_account(accounts: Vec<Account>, refresh_token: &str) -> Result<Account, String> {
    accounts
        .into_iter()
        .find(|account| !refresh_token.is_empty() && account.token.refresh_token == refresh_token)
        .ok_or_else(|| {
            "The agy keyring credentials do not match a managed account; import the current login before syncing.".into()
        })
}

/// Add account (向后兼容薄封装，直接委托给 upsert_account 保证严格幂等性，杜绝重复凭据文件)
pub fn add_account(
    email: String,
    name: Option<String>,
    token: TokenData,
) -> Result<Account, String> {
    upsert_account(email, name, token)
}

/// Add or update account (严格幂等主入口)
pub fn upsert_account(
    email: String,
    name: Option<String>,
    token: TokenData,
) -> Result<Account, String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
    let mut index = load_account_index()?;

    let norm_email = email.trim().to_lowercase();
    let norm_rt = token.refresh_token.trim().to_string();

    // 幂等查找：按邮箱（忽略大小写）或凭据查找已有账号 ID
    let existing_account_id = index
        .accounts
        .iter()
        .find(|s| s.email.trim().to_lowercase() == norm_email)
        .map(|s| s.id.clone());

    if let Some(account_id) = existing_account_id {
        // 原地复用现有账号 ID，覆盖并更新，绝不生成新的文件！
        match load_account(&account_id) {
            Ok(mut account) => {
                let old_access_token = account.token.access_token.clone();
                let old_refresh_token = account.token.refresh_token.clone();
                account.token = token;
                account.name = name.clone();
                if account.disabled
                    && (account.token.refresh_token != old_refresh_token
                        || account.token.access_token != old_access_token)
                {
                    account.disabled = false;
                    account.disabled_reason = None;
                    account.disabled_at = None;
                }
                account.update_last_used();
                save_account(&account)?;

                // 同步索引
                if let Some(idx_summary) = index.accounts.iter_mut().find(|s| s.id == account_id) {
                    idx_summary.name = name;
                    idx_summary.email = account.email.clone();
                    save_account_index(&index)?;
                }

                return Ok(account);
            }
            Err(e) => {
                crate::modules::logger::log_warn(&format!(
                    "Account {} file missing ({}), recreating...",
                    account_id, e
                ));
                let mut account = Account::new(account_id.clone(), email.clone(), token);
                account.name = name.clone();
                save_account(&account)?;

                if let Some(idx_summary) = index.accounts.iter_mut().find(|s| s.id == account_id) {
                    idx_summary.name = name;
                    idx_summary.email = account.email.clone();
                    save_account_index(&index)?;
                }

                return Ok(account);
            }
        }
    }

    // 账号确实不存在，创建新账号
    let account_id = Uuid::new_v4().to_string();
    let mut account = Account::new(account_id.clone(), email.clone(), token);
    account.name = name.clone();
    save_account(&account)?;

    index.accounts.push(AccountSummary {
        id: account.id.clone(),
        email: account.email.clone(),
        name: account.name.clone(),
        disabled: account.disabled,
        proxy_disabled: account.proxy_disabled,
        protected_models: account.protected_models.clone(),
        created_at: account.created_at,
        last_used: account.last_used,
    });

    if index.current_account_id.is_none() {
        index.current_account_id = Some(account_id);
    }

    save_account_index(&index)?;
    Ok(account)
}

/// Delete account
pub fn delete_account(account_id: &str) -> Result<(), String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
    let mut index = load_account_index()?;

    // Remove from index (capture email before retain so live model cache can be pruned too)
    let deleted_email = index
        .accounts
        .iter()
        .find(|s| s.id == account_id)
        .map(|s| s.email.clone())
        .or_else(|| load_account(account_id).ok().map(|acc| acc.email));
    let original_len = index.accounts.len();
    index.accounts.retain(|s| s.id != account_id);
    let index_modified = index.accounts.len() != original_len;

    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account_id));
    let file_existed = account_path.exists();

    // 既未在索引中匹配，磁盘上也不存在实体文件，才报错未找到
    if !index_modified && !file_existed {
        return Err(format!("Account ID not found: {}", account_id));
    }

    // Clear current account if it's being deleted
    if index.current_account_id.as_deref() == Some(account_id) {
        index.current_account_id = index.accounts.first().map(|s| s.id.clone());
    }

    // [FIX #3806, #3808] 失败点前置：先写索引（若索引发生变更），成功后再删除物理文件
    if index_modified {
        save_account_index(&index)?;
    }

    // 无论索引中是否存在，均尝试删除物理凭据文件（兜底清理孤儿文件）；删除失败通过 ? 向上抛错
    if file_existed {
        fs::remove_file(&account_path)
            .map_err(|e| format!("failed_to_delete_account_file: {}", e))?;
    }

    if let Some(email) = deleted_email {
        crate::models::OfficialModelCatalog::remove_live_account(&email);
    }

    // [FIX #1477] Trigger TokenManager cache cleanup signal
    crate::proxy::server::trigger_account_delete(account_id);

    Ok(())
}

/// Batch delete accounts (atomic index operation)
pub fn delete_accounts(account_ids: &[String]) -> Result<(), String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
    let mut index = load_account_index()?;

    let accounts_dir = get_accounts_dir()?;
    let deleted_emails: Vec<String> = account_ids
        .iter()
        .filter_map(|id| {
            index
                .accounts
                .iter()
                .find(|s| &s.id == id)
                .map(|s| s.email.clone())
                .or_else(|| load_account(id).ok().map(|acc| acc.email))
        })
        .collect();

    for account_id in account_ids {
        // Remove from index
        index.accounts.retain(|s| &s.id != account_id);

        // Clear current account if it's being deleted
        if index.current_account_id.as_deref() == Some(account_id) {
            index.current_account_id = None;
        }

        // Delete account file
        let account_path = accounts_dir.join(format!("{}.json", account_id));
        if account_path.exists() {
            let _ = fs::remove_file(&account_path);
        }

        // [FIX #1477] Trigger TokenManager cache cleanup signal
        crate::proxy::server::trigger_account_delete(account_id);
    }

    for email in deleted_emails {
        crate::models::OfficialModelCatalog::remove_live_account(&email);
    }

    // If current account is empty, use first one as default
    if index.current_account_id.is_none() {
        index.current_account_id = index.accounts.first().map(|s| s.id.clone());
    }

    save_account_index(&index)
}

/// Reorder account list
/// Update account order in index file based on provided IDs
pub fn reorder_accounts(account_ids: &[String]) -> Result<(), String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
    let mut index = load_account_index()?;

    // Create a map of account ID to summary
    let id_to_summary: std::collections::HashMap<_, _> = index
        .accounts
        .iter()
        .map(|s| (s.id.clone(), s.clone()))
        .collect();

    // Rebuild account list with new order
    let mut new_accounts = Vec::new();
    for id in account_ids {
        if let Some(summary) = id_to_summary.get(id) {
            new_accounts.push(summary.clone());
        }
    }

    // Add accounts missing from new order to the end
    for summary in &index.accounts {
        if !account_ids.contains(&summary.id) {
            new_accounts.push(summary.clone());
        }
    }

    index.accounts = new_accounts;

    crate::modules::logger::log_info(&format!(
        "Account order updated, {} accounts total",
        index.accounts.len()
    ));

    save_account_index(&index)
}

/// Switch current account (Core Logic)
pub async fn switch_account(
    account_id: &str,
    target_ide: Option<&str>,
    integration: &(impl modules::integration::SystemIntegration + ?Sized),
) -> Result<(), String> {
    use crate::modules::oauth;

    let index = {
        let _lock = ACCOUNT_INDEX_LOCK
            .lock()
            .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
        load_account_index()?
    };

    // 1. Verify account exists
    if !index.accounts.iter().any(|s| s.id == account_id) {
        return Err(format!("Account not found: {}", account_id));
    }

    let mut account = load_account(account_id)?;
    crate::modules::logger::log_info(&format!(
        "Switching to account: {} (ID: {}) (target_ide: {:?})",
        account.email, account.id, target_ide
    ));

    // 2. Ensure token is valid before switch. Surface clearer hints for known account-state failures.
    let fresh_token = match oauth::ensure_fresh_token(&account.token, Some(&account.id)).await {
        Ok(token) => token,
        Err(e) => {
            if is_account_access_blocked_message(&e) {
                mark_validation_blocked(&mut account, &e);
            }
            return Err(format_switch_refresh_error(&e));
        }
    };

    // If Token updated, save back to account file
    if fresh_token.access_token != account.token.access_token {
        account.token = fresh_token.clone();
        save_account(&account)?;
    }

    ensure_enterprise_project_ready(&mut account).await?;

    // [FIX] Ensure account has a device profile for isolation
    if account.device_profile.is_none() {
        crate::modules::logger::log_info(&format!(
            "Account {} has no bound fingerprint, generating new one for isolation...",
            account.email
        ));
        let new_profile = modules::device::generate_profile();
        apply_profile_to_account(
            &mut account,
            new_profile.clone(),
            Some("auto_generated".to_string()),
            true,
        )?;
    }

    // 3. Execute platform-specific system integration (Close proc, Inject DB, Start proc, etc.)
    integration.on_account_switch(&account, target_ide).await?;

    // 4. Update tool internal state
    set_current_account_id_with_target(account_id, target_ide)?;

    account.update_last_used();
    save_account(&account)?;

    crate::modules::logger::log_info(&format!(
        "Account switch core logic completed: {}",
        account.email
    ));

    Ok(())
}

fn is_enterprise_client(client_key: Option<&str>) -> bool {
    crate::modules::oauth::is_gcp_enterprise_client(client_key)
        || client_key
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(|key| key.eq_ignore_ascii_case("antigravity_enterprise"))
            .unwrap_or(false)
}

fn normalize_project_id(project_id: Option<&str>) -> Option<String> {
    project_id
        .map(str::trim)
        .filter(|pid| !pid.is_empty())
        .map(ToOwned::to_owned)
}

async fn ensure_enterprise_project_ready(account: &mut Account) -> Result<(), String> {
    if !is_enterprise_client(account.token.oauth_client_key.as_deref()) {
        return Ok(());
    }

    if normalize_project_id(account.token.project_id.as_deref()).is_some() {
        return Ok(());
    }

    crate::modules::logger::log_warn(&format!(
        "Account {} is using enterprise OAuth client but missing project_id. Trying to resolve before switch...",
        account.email
    ));

    match crate::proxy::project_resolver::fetch_project_id(&account.token.access_token).await {
        Ok(project_id) => {
            crate::modules::logger::log_info(&format!(
                "Resolved enterprise project_id for {}: {}",
                account.email, project_id
            ));
            account.token.project_id = Some(project_id);
            save_account(account)?;
            Ok(())
        }
        Err(e) => {
            crate::modules::logger::log_warn(&format!(
                "Account {} is currently missing enterprise project_id and auto-resolve failed ({}). Allowing switch to proceed, but certain enterprise features may be limited.",
                account.email, e
            ));
            Ok(())
        }
    }
}

fn is_rate_limit_error(err: &crate::error::AppError) -> bool {
    match err {
        crate::error::AppError::Network(_, Some(status)) => *status == 429,
        crate::error::AppError::Unknown(msg)
        | crate::error::AppError::OAuth(msg)
        | crate::error::AppError::Account(msg)
        | crate::error::AppError::Config(msg) => {
            let lower = msg.to_lowercase();
            lower.contains("429")
                || lower.contains("too many requests")
                || lower.contains("resource_exhausted")
                || lower.contains("resource has been exhausted")
        }
        _ => false,
    }
}

fn recover_cached_quota_on_rate_limit(
    account: &Account,
    err: &crate::error::AppError,
) -> Option<QuotaData> {
    if !is_rate_limit_error(err) {
        return None;
    }

    let cached = account.quota.clone()?;
    if cached.models.is_empty() {
        return None;
    }

    Some(cached)
}

fn is_validation_required_error(err: &crate::error::AppError) -> bool {
    let text = err.to_string().to_lowercase();
    text.contains("verify your account")
        || text.contains("further action is required")
        || text.contains("validation_url")
        || text.contains("appeal_url")
        || text.contains("validation required")
        || text.contains("validation_required")
}

pub fn extract_action_url(reason: &str) -> Option<String> {
    fn normalize(raw: &str) -> String {
        raw.replace("\\u0026", "&")
            .replace("\\u003d", "=")
            .replace("\\/", "/")
            .trim()
            .to_string()
    }

    fn from_value(parsed: &serde_json::Value) -> Option<String> {
        let error = parsed.get("error").unwrap_or(parsed);

        // 申诉链接优先（TOS 封禁），其次才是验证链接。
        if let Some(url) = error
            .get("details")
            .and_then(|d| d.as_array())
            .and_then(|arr| arr.iter().find_map(|item| item.get("metadata")))
            .and_then(|m| m.get("appeal_url").or_else(|| m.get("validation_url")))
            .and_then(|u| u.as_str())
        {
            return Some(normalize(url));
        }

        if let Some(url) = error
            .get("details")
            .and_then(|d| d.as_array())
            .and_then(|arr| arr.iter().find_map(|item| item.get("links")))
            .and_then(|links| links.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("url"))
            .and_then(|u| u.as_str())
        {
            return Some(normalize(url));
        }

        None
    }

    // 调用方可能给 JSON 拼上 "Forbidden (403): " 前缀，因此允许从任意文本中
    // 定位内嵌的 JSON 对象，而不是只接受纯 JSON 字符串。
    let parsed = serde_json::from_str::<serde_json::Value>(reason)
        .ok()
        .or_else(|| {
            let start = reason.find('{')?;
            let end = reason.rfind('}')?;
            if end <= start {
                return None;
            }
            serde_json::from_str::<serde_json::Value>(&reason[start..=end]).ok()
        });

    if let Some(parsed) = parsed {
        if let Some(url) = from_value(&parsed) {
            return Some(url);
        }

        if let Some(inner) = parsed.get("error").and_then(|v| v.as_str()) {
            if let Ok(inner_parsed) = serde_json::from_str::<serde_json::Value>(inner) {
                if let Some(url) = from_value(&inner_parsed) {
                    return Some(url);
                }
            }
        }
    }

    // 最后兜底：完整提取（支持 https:\/\/... 转义，且不会在 & 或反斜杠处截断）。
    AccountBanInfo::extract_url_from_text(reason)
}

fn is_account_access_blocked_message(message: &str) -> bool {
    let text = message.to_lowercase();
    text.contains("verify your account")
        || text.contains("further action is required")
        || text.contains("validation_url")
        || text.contains("appeal_url")
        || text.contains("validation required")
        || text.contains("validation_required")
        || text.contains("unauthorized_client")
        || text.contains("invalid_client")
        || text.contains("invalid_grant")
        || text.contains("resource_exhausted")
        || text.contains("resource has been exhausted")
}

fn format_switch_refresh_error(message: &str) -> String {
    let lower = message.to_lowercase();

    if lower.contains("unauthorized_client")
        || lower.contains("invalid_client")
        || lower.contains("invalid_grant")
    {
        return format!(
            "Token refresh failed: OAuth client is not authorized for this account. Please sign in again in Antigravity-Manager and complete authorization/verification. Raw error: {}",
            message
        );
    }

    if lower.contains("verify your account")
        || lower.contains("further action is required")
        || lower.contains("validation_url")
        || lower.contains("appeal_url")
        || lower.contains("validation required")
        || lower.contains("validation_required")
    {
        return format!(
            "Token refresh failed: account requires additional verification. Please finish verification in Antigravity, then retry account switch. Raw error: {}",
            message
        );
    }

    if lower.contains("resource_exhausted") || lower.contains("resource has been exhausted") {
        return format!(
            "Token refresh failed: account is rate-limited or temporarily restricted (RESOURCE_EXHAUSTED). Please retry later. Raw error: {}",
            message
        );
    }

    format!("Token refresh failed: {}", message)
}

fn format_rate_limit_block_reason(err: &crate::error::AppError) -> String {
    format!(
        "Account is temporarily rate-limited or risk-controlled (RESOURCE_EXHAUSTED). Please cool down and retry later. Raw error: {}",
        err
    )
}

fn mark_validation_blocked(account: &mut Account, reason: &str) {
    let now = chrono::Utc::now().timestamp();
    let block_until = now + (10 * 60);
    let extracted_url = extract_action_url(reason);

    if account.validation_blocked
        && account.validation_blocked_reason.as_deref() == Some(reason)
        && account.validation_url == extracted_url
    {
        return;
    }

    account.validation_blocked = true;
    account.validation_blocked_until = Some(block_until);
    account.validation_blocked_reason = Some(reason.to_string());
    if let Some(url) = extracted_url {
        account.validation_url = Some(url);
    }
    if let Some(ref mut q) = account.quota {
        q.is_forbidden = true;
        let existing_kind = classify_lock_reason(q.forbidden_reason.as_deref().unwrap_or(""));
        if !matches!(
            existing_kind,
            AccountLockKind::HardForbidden | AccountLockKind::Banned
        ) {
            q.forbidden_reason = Some(reason.to_string());
        }
    }

    if let Err(e) = save_account(account) {
        crate::modules::logger::log_warn(&format!(
            "Failed to persist validation_blocked state for {}: {}",
            account.email, e
        ));
    }
}

fn clear_validation_blocked(account: &mut Account, force: bool) {
    if !account.validation_blocked
        && account.validation_blocked_until.is_none()
        && account.validation_blocked_reason.is_none()
        && account.validation_url.is_none()
    {
        return;
    }

    let now = chrono::Utc::now().timestamp();
    if !force && account.is_validation_block_active(now) {
        // 关键守卫：验证锁仍在有效冷静期内，非强制调用（如日常配额 200）严禁抹除验证状态！
        return;
    }

    account.validation_blocked = false;
    account.validation_blocked_until = None;
    account.validation_blocked_reason = None;
    account.validation_url = None;

    // 严禁因人机验证锁清除而越权清除 403 Forbidden 或 TOS 封禁状态。
    // 只有定向测活成功 (clear_account_forbidden_and_validation) 或用户手动操作才能解封 403。
    // 同时必须只看“仍然生效”的锁：proxy_disabled / quota.is_forbidden 为 false 时，
    // 陈旧的 reason 文本不得阻止验证状态清理。
    let is_hard_lock = account.banned
        || (account.proxy_disabled
            && account
                .proxy_disabled_reason
                .as_deref()
                .is_some_and(|reason| {
                    matches!(
                        classify_lock_reason(reason),
                        AccountLockKind::Banned | AccountLockKind::HardForbidden
                    )
                }))
        || account.quota.as_ref().is_some_and(|quota| {
            quota.is_forbidden
                && quota.forbidden_reason.as_deref().is_some_and(|reason| {
                    matches!(
                        classify_lock_reason(reason),
                        AccountLockKind::Banned | AccountLockKind::HardForbidden
                    )
                })
        });

    if !is_hard_lock {
        if let Some(ref mut q) = account.quota {
            q.is_forbidden = false;
            q.forbidden_reason = None;
        }

        if account.proxy_disabled
            && account
                .proxy_disabled_reason
                .as_deref()
                .is_some_and(|reason| classify_lock_reason(reason) == AccountLockKind::Validation)
        {
            if account.manual_proxy_disabled {
                account.proxy_disabled = true;
                account.proxy_disabled_reason = Some("manual".to_string());
            } else {
                account.proxy_disabled = false;
                account.proxy_disabled_reason = None;
                account.proxy_disabled_at = None;
            }
        }
    }

    if let Err(e) = save_account(account) {
        crate::modules::logger::log_warn(&format!(
            "Failed to clear validation_blocked state for {}: {}",
            account.email, e
        ));
    }
}

/// Get device profile info: current storage.json + account bound profile
#[derive(Debug, Serialize)]
pub struct DeviceProfiles {
    pub current_storage: Option<DeviceProfile>,
    pub bound_profile: Option<DeviceProfile>,
    pub history: Vec<DeviceProfileVersion>,
    pub baseline: Option<DeviceProfile>,
}

pub fn get_device_profiles(account_id: &str) -> Result<DeviceProfiles, String> {
    // In headless/Docker mode, storage.json may not exist - handle gracefully
    let current = crate::modules::device::get_storage_path(None)
        .ok()
        .and_then(|path| crate::modules::device::read_profile(&path).ok());
    let account = load_account(account_id)?;
    Ok(DeviceProfiles {
        current_storage: current,
        bound_profile: account.device_profile.clone(),
        history: account.device_history.clone(),
        baseline: crate::modules::device::load_global_original(),
    })
}

/// Bind device profile and write to storage.json immediately
pub fn bind_device_profile(account_id: &str, mode: &str) -> Result<DeviceProfile, String> {
    use crate::modules::device;

    let profile = match mode {
        "capture" => device::read_profile(&device::get_storage_path(None)?)?,
        "generate" => device::generate_profile(),
        _ => return Err("mode must be 'capture' or 'generate'".to_string()),
    };

    let mut account = load_account(account_id)?;
    let _ = device::save_global_original(&profile);
    apply_profile_to_account(&mut account, profile.clone(), Some(mode.to_string()), true)?;

    Ok(profile)
}

/// Bind directly with provided profile
pub fn bind_device_profile_with_profile(
    account_id: &str,
    profile: DeviceProfile,
    label: Option<String>,
) -> Result<DeviceProfile, String> {
    let mut account = load_account(account_id)?;
    let _ = crate::modules::device::save_global_original(&profile);
    apply_profile_to_account(&mut account, profile.clone(), label, true)?;

    Ok(profile)
}

fn apply_profile_to_account(
    account: &mut Account,
    profile: DeviceProfile,
    label: Option<String>,
    add_history: bool,
) -> Result<(), String> {
    account.device_profile = Some(profile.clone());
    if add_history {
        // Clear 'current' flag
        for h in account.device_history.iter_mut() {
            h.is_current = false;
        }
        account.device_history.push(DeviceProfileVersion {
            id: Uuid::new_v4().to_string(),
            created_at: chrono::Utc::now().timestamp(),
            label: label.unwrap_or_else(|| "generated".to_string()),
            profile: profile.clone(),
            is_current: true,
        });
    }
    save_account(account)?;
    Ok(())
}

/// List available device profile versions for an account (including baseline)
pub fn list_device_versions(account_id: &str) -> Result<DeviceProfiles, String> {
    get_device_profiles(account_id)
}

/// Restore device profile by version ID ("baseline" for global original, "current" for current bound)
pub fn restore_device_version(account_id: &str, version_id: &str) -> Result<DeviceProfile, String> {
    let mut account = load_account(account_id)?;

    let target_profile = if version_id == "baseline" {
        crate::modules::device::load_global_original().ok_or("Global original profile not found")?
    } else if let Some(v) = account.device_history.iter().find(|v| v.id == version_id) {
        v.profile.clone()
    } else if version_id == "current" {
        account
            .device_profile
            .clone()
            .ok_or("No currently bound profile")?
    } else {
        return Err("Device profile version not found".to_string());
    };

    account.device_profile = Some(target_profile.clone());
    for h in account.device_history.iter_mut() {
        h.is_current = h.id == version_id;
    }
    save_account(&account)?;
    Ok(target_profile)
}

/// Delete specific historical device profile (baseline cannot be deleted)
pub fn delete_device_version(account_id: &str, version_id: &str) -> Result<(), String> {
    if version_id == "baseline" {
        return Err("Original profile cannot be deleted".to_string());
    }
    let mut account = load_account(account_id)?;
    if account
        .device_history
        .iter()
        .any(|v| v.id == version_id && v.is_current)
    {
        return Err("Currently bound profile cannot be deleted".to_string());
    }
    let before = account.device_history.len();
    account.device_history.retain(|v| v.id != version_id);
    if account.device_history.len() == before {
        return Err("Historical device profile not found".to_string());
    }
    save_account(&account)?;
    Ok(())
}
/// Apply account bound device profile to storage.json
pub fn apply_device_profile(account_id: &str) -> Result<DeviceProfile, String> {
    use crate::modules::device;
    let mut account = load_account(account_id)?;
    let profile = account
        .device_profile
        .clone()
        .ok_or("Account has no bound device profile")?;
    let storage_path = device::get_storage_path(None)?;
    device::write_profile(&storage_path, &profile)?;
    account.update_last_used();
    save_account(&account)?;
    Ok(profile)
}

/// Restore earliest storage.json backup (approximate "original" state)
pub fn restore_original_device() -> Result<String, String> {
    if let Some(current_id) = get_current_account_id()? {
        if let Ok(mut account) = load_account(&current_id) {
            if let Some(original) = crate::modules::device::load_global_original() {
                account.device_profile = Some(original);
                for h in account.device_history.iter_mut() {
                    h.is_current = false;
                }
                save_account(&account)?;
                return Ok(
                    "Reset current account bound profile to original (not applied to storage)"
                        .to_string(),
                );
            }
        }
    }
    Err("Original profile not found, cannot restore".to_string())
}

/// Get current account ID
pub fn get_current_account_id() -> Result<Option<String>, String> {
    let index = load_account_index()?;
    Ok(index.current_account_id)
}

/// Get currently active account details
pub fn get_current_account() -> Result<Option<Account>, String> {
    if let Some(id) = get_current_account_id()? {
        Ok(Some(load_account(&id)?))
    } else {
        Ok(None)
    }
}

/// Set current active account ID
pub fn set_current_account_id(account_id: &str) -> Result<(), String> {
    set_current_account_id_with_target(account_id, None)
}

/// Set current active account ID and target IDE
pub fn set_current_account_id_with_target(
    account_id: &str,
    target_ide: Option<&str>,
) -> Result<(), String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
    let mut index = load_account_index()?;
    index.current_account_id = Some(account_id.to_string());
    let normalized_target = if crate::modules::integration::is_acp_target(target_ide) {
        Some("acp")
    } else {
        target_ide
    };
    index.current_target_ide = normalized_target.map(|s| s.to_string());
    match normalized_target {
        Some("classic") => index.current_classic_account_id = Some(account_id.to_string()),
        Some("ide") => index.current_ide_account_id = Some(account_id.to_string()),
        Some("agy") => index.current_agy_account_id = Some(account_id.to_string()),
        Some("acp") => index.current_acp_account_id = Some(account_id.to_string()),
        _ => {
            index.current_classic_account_id = Some(account_id.to_string());
        }
    }
    save_account_index(&index)
}

/// 智能探查并获取经典款、Antigravity IDE、Antigravity CLI、IDE Plugin 四大客户端当前各自生效的账号状态
pub fn detect_active_clients_status() -> Result<crate::models::ActiveClientsStatus, String> {
    let index = load_account_index()?;
    let mut status = crate::models::ActiveClientsStatus {
        classic_account_id: index.current_classic_account_id.clone(),
        classic_email: None,
        ide_account_id: index.current_ide_account_id.clone(),
        ide_email: None,
        agy_account_id: index.current_agy_account_id.clone(),
        agy_email: None,
        acp_account_id: index.current_acp_account_id.clone(),
        acp_email: None,
    };

    // [FIX #3726] 针对探测出的外部 refresh_token 进行快速按需匹配（彻底杜绝全量加载数百个小文件与 SQLite 密集查询）
    let find_email_by_id = |id: &str| -> Option<String> {
        index
            .accounts
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.email.clone())
    };

    let find_by_rt = |rt: &str| -> Option<(String, String)> {
        if rt.is_empty() {
            return None;
        }

        // 优先在当前已知的候选账号中极速比对（绝大多数情况下命中，仅需读取 1~2 个小文件）
        let candidates = [
            index.current_ide_account_id.as_deref(),
            index.current_classic_account_id.as_deref(),
            index.current_agy_account_id.as_deref(),
            index.current_acp_account_id.as_deref(),
            index.current_account_id.as_deref(),
        ];
        for candidate_id in candidates.into_iter().flatten() {
            if let Ok(acc) = load_account(candidate_id) {
                if acc.token.refresh_token == rt {
                    return Some((acc.id, acc.email));
                }
            }
        }

        // 未命中候选时，遍历索引进行懒加载匹配，找到即停
        for summary in &index.accounts {
            if let Ok(acc) = load_account(&summary.id) {
                if acc.token.refresh_token == rt {
                    return Some((acc.id, acc.email));
                }
            }
        }
        None
    };

    // 1. 探测 IDE 款当前账号 (state.vscdb)
    if let Ok(db_path) = crate::modules::db::get_db_path(Some("ide")) {
        if db_path.exists() {
            if let Ok(oauth_state) =
                crate::modules::migration::extract_oauth_state_from_file(&db_path)
            {
                if let Some((acc_id, acc_email)) = find_by_rt(&oauth_state.refresh_token) {
                    status.ide_account_id = Some(acc_id);
                    status.ide_email = Some(acc_email);
                }
            }
        }
    }
    if status.ide_account_id.is_none() {
        if let Some(ref id) = index.current_ide_account_id {
            status.ide_account_id = Some(id.clone());
            status.ide_email = find_email_by_id(id);
        }
    } else if status.ide_email.is_none() {
        if let Some(ref id) = status.ide_account_id {
            if let Some(email) = find_email_by_id(id) {
                status.ide_email = Some(email);
            }
        }
    }

    // 2. 读取底层凭据
    // 系统钥匙串（Keyring / Secret Service）：Antigravity 2.0+ 原生经典款与 CLI 共享的核心凭据
    let keyring_acc = crate::modules::integration::read_from_system_keyring_only()
        .ok()
        .and_then(|st| find_by_rt(&st.refresh_token));

    // CLI 专属独立文件凭据 (~/.gemini/antigravity-cli/antigravity-oauth-token)
    let cli_file_acc = crate::modules::integration::read_antigravity_cli_credentials()
        .first()
        .and_then(|st| find_by_rt(&st.refresh_token));

    // 经典款旧版 SQLite 数据库 (Antigravity < 2.0.0)
    let classic_db_acc = crate::modules::db::get_db_path(Some("classic"))
        .ok()
        .filter(|p| p.exists())
        .and_then(|p| crate::modules::migration::extract_oauth_state_from_file(&p).ok())
        .and_then(|st| find_by_rt(&st.refresh_token));

    // 判断经典款是否为 2.0+ 原生版本（>= 2.0.0 使用 Keyring，< 2.0.0 使用 SQLite）
    let is_classic_v2 =
        crate::modules::version::get_antigravity_version_with_path(Some("classic"), None)
            .map(|v| {
                crate::modules::version::compare_version(&v.short_version, "2.0.0")
                    != std::cmp::Ordering::Less
            })
            .unwrap_or(true);

    // 3. 确定经典款当前账号
    if is_classic_v2 {
        if let Some((ref k_id, ref k_email)) = keyring_acc {
            if index.current_target_ide.as_deref() != Some("agy")
                || index.current_classic_account_id.as_deref() == Some(k_id)
            {
                status.classic_account_id = Some(k_id.clone());
                status.classic_email = Some(k_email.clone());
            }
        }
    } else if let Some((ref db_id, ref db_email)) = classic_db_acc {
        status.classic_account_id = Some(db_id.clone());
        status.classic_email = Some(db_email.clone());
    }
    if status.classic_account_id.is_none() {
        if let Some(ref id) = index.current_classic_account_id {
            status.classic_account_id = Some(id.clone());
            status.classic_email = find_email_by_id(id);
        }
    } else if status.classic_email.is_none() {
        if let Some(ref id) = status.classic_account_id {
            if let Some(email) = find_email_by_id(id) {
                status.classic_email = Some(email);
            }
        }
    }

    // 4. 确定 CLI (agy) 当前账号
    if let Some((ref c_id, ref c_email)) = cli_file_acc {
        if index.current_target_ide.as_deref() == Some("agy")
            || index.current_agy_account_id.as_deref() == Some(c_id)
        {
            status.agy_account_id = Some(c_id.clone());
            status.agy_email = Some(c_email.clone());
        }
    }
    if status.agy_email.is_none() {
        if let Some((ref k_id, ref k_email)) = keyring_acc {
            if index.current_target_ide.as_deref() == Some("agy")
                || index.current_agy_account_id.as_deref() == Some(k_id)
            {
                status.agy_account_id = Some(k_id.clone());
                status.agy_email = Some(k_email.clone());
            }
        }
    }
    if status.agy_account_id.is_none() {
        if let Some(ref id) = index.current_agy_account_id {
            status.agy_account_id = Some(id.clone());
            status.agy_email = find_email_by_id(id);
        }
    } else if status.agy_email.is_none() {
        if let Some(ref id) = status.agy_account_id {
            if let Some(email) = find_email_by_id(id) {
                status.agy_email = Some(email);
            }
        }
    }

    // 5. Determine active account for IDE Plugin (antigravity-acp)
    let acp_acc = crate::modules::integration::read_acp_credentials()
        .ok()
        .and_then(|st| find_by_rt(&st.refresh_token));
    if let Some((a_id, a_email)) = acp_acc {
        status.acp_account_id = Some(a_id);
        status.acp_email = Some(a_email);
    }
    if status.acp_account_id.is_none() {
        if let Some(ref id) = index.current_acp_account_id {
            status.acp_account_id = Some(id.clone());
            status.acp_email = find_email_by_id(id);
        }
    } else if status.acp_email.is_none() {
        if let Some(ref id) = status.acp_account_id {
            if let Some(email) = find_email_by_id(id) {
                status.acp_email = Some(email);
            }
        }
    }

    // Fallback if index has current_account_id and current_target_ide
    if status.classic_account_id.is_none()
        && status.ide_account_id.is_none()
        && status.agy_account_id.is_none()
        && status.acp_account_id.is_none()
    {
        if let Some(ref curr_id) = index.current_account_id {
            match index.current_target_ide.as_deref() {
                Some("ide") => status.ide_account_id = Some(curr_id.clone()),
                Some("agy") => status.agy_account_id = Some(curr_id.clone()),
                Some("acp" | "plugin" | "ide-plugin") => {
                    status.acp_account_id = Some(curr_id.clone())
                }
                _ => status.classic_account_id = Some(curr_id.clone()),
            }
        }
    }

    Ok(status)
}

/// 智能解析目标账号：支持 UUID、邮箱（完整或前缀）、账号名称、1-based 序号，或 None 时顺延下一个未禁用账号
pub fn resolve_account_summary(
    query: Option<&str>,
    target_ide: Option<&str>,
) -> Result<crate::models::AccountSummary, String> {
    let index = load_account_index()?;
    resolve_account_summary_in_index(&index, query, target_ide)
}

/// 基于传入的 AccountIndex 内部解析目标账号（纯函数，便于单元测试）
pub fn resolve_account_summary_in_index(
    index: &crate::models::AccountIndex,
    query: Option<&str>,
    target_ide: Option<&str>,
) -> Result<crate::models::AccountSummary, String> {
    if index.accounts.is_empty() {
        return Err("当前无可用账号".to_string());
    }

    let trimmed = query.map(str::trim).filter(|s| !s.is_empty());

    if let Some(q) = trimmed {
        // 1. 如果是纯数字，按 1-based 序号查找
        if let Ok(idx) = q.parse::<usize>() {
            if idx >= 1 && idx <= index.accounts.len() {
                return Ok(index.accounts[idx - 1].clone());
            }
        }

        // 2. 精确匹配 ID
        if let Some(acc) = index.accounts.iter().find(|a| a.id == q) {
            return Ok(acc.clone());
        }

        // 3. 精确匹配 Email（忽略大小写）
        if let Some(acc) = index
            .accounts
            .iter()
            .find(|a| a.email.eq_ignore_ascii_case(q))
        {
            return Ok(acc.clone());
        }

        // 4. 精确匹配 Name（别名）
        if let Some(acc) = index.accounts.iter().find(|a| a.name.as_deref() == Some(q)) {
            return Ok(acc.clone());
        }

        // 5. 模糊包含匹配 Email
        let q_lower = q.to_lowercase();
        if let Some(acc) = index
            .accounts
            .iter()
            .find(|a| a.email.to_lowercase().contains(&q_lower))
        {
            return Ok(acc.clone());
        }

        // 6. 模糊包含匹配 Name
        if let Some(acc) = index.accounts.iter().find(|a| {
            a.name
                .as_deref()
                .map(|n| n.to_lowercase().contains(&q_lower))
                .unwrap_or(false)
        }) {
            return Ok(acc.clone());
        }

        return Err(format!("未找到匹配的账号: '{}'", q));
    }

    // 无参时：环形顺延轮换（Round-Robin）
    let current_id = match target_ide {
        Some("agy") => index
            .current_agy_account_id
            .as_deref()
            .or(index.current_account_id.as_deref()),
        Some("acp" | "plugin" | "ide-plugin") => index
            .current_acp_account_id
            .as_deref()
            .or(index.current_account_id.as_deref()),
        Some("ide") => index
            .current_ide_account_id
            .as_deref()
            .or(index.current_account_id.as_deref()),
        Some("classic") => index
            .current_classic_account_id
            .as_deref()
            .or(index.current_account_id.as_deref()),
        _ => index.current_account_id.as_deref(),
    };

    let total = index.accounts.len();
    let current_idx = current_id
        .and_then(|id| index.accounts.iter().position(|a| a.id == id))
        .unwrap_or(0);

    // 优先从下一个开始寻找未被禁用的账号
    for offset in 1..=total {
        let idx = (current_idx + offset) % total;
        let candidate = &index.accounts[idx];
        if !candidate.disabled && !candidate.proxy_disabled {
            return Ok(candidate.clone());
        }
    }

    // 保底：寻找任意未禁用的账号
    for offset in 1..=total {
        let idx = (current_idx + offset) % total;
        let candidate = &index.accounts[idx];
        if !candidate.disabled {
            return Ok(candidate.clone());
        }
    }

    // 若都不可用，返回第一个
    Ok(index.accounts[0].clone())
}

/// Update account quota with optional immediate index sync
pub fn update_account_quota_internal(
    account_id: &str,
    quota: QuotaData,
    sync_index: bool,
) -> Result<Account, String> {
    let _account_write = lock_account_file_updates()?;
    let mut account = load_account(account_id)?;
    account.update_quota(quota);

    // 封禁状态具有比配额快照更高的权威性：Daily 端点成功返回配额并不代表解封。
    // 在权威探测清除 `banned` 之前，始终保持反代禁用与 is_forbidden。
    if account.banned {
        account.proxy_disabled = true;
        if !account
            .proxy_disabled_reason
            .as_deref()
            .map_or(false, |reason| reason.starts_with("banned:"))
        {
            account.proxy_disabled_reason = Some(format!(
                "banned: {}",
                account.banned_code.as_deref().unwrap_or("BANNED")
            ));
        }
        if account.proxy_disabled_at.is_none() {
            account.proxy_disabled_at = Some(chrono::Utc::now().timestamp());
        }
    }

    // --- Quota protection logic start ---
    if let Ok(config) = crate::modules::config::load_app_config() {
        if config.quota_protection.enabled {
            if let Some(ref q) = account.quota {
                let threshold = config.quota_protection.threshold_percentage as i32;

                let mut group_max_percentage: HashMap<String, i32> = HashMap::new();

                for model in &q.models {
                    if let Some(std_id) =
                        crate::proxy::common::model_mapping::normalize_to_standard_id(&model.name)
                    {
                        let entry = group_max_percentage.entry(std_id).or_insert(-1);
                        if model.percentage > *entry {
                            *entry = model.percentage;
                        }
                    }
                }

                for std_id in &config.quota_protection.monitored_models {
                    let lookup_key =
                        crate::proxy::common::model_mapping::normalize_to_standard_id(std_id)
                            .unwrap_or_else(|| std_id.clone());
                    let max_pct = group_max_percentage
                        .get(&lookup_key)
                        .cloned()
                        .unwrap_or(100);

                    if max_pct < threshold {
                        if !account.protected_models.contains(&lookup_key) {
                            crate::modules::logger::log_info(&format!(
                                "[Quota] Triggering model protection: {} (Group: {} Max: {}% < Thres: {}%)",
                                account.email, lookup_key, max_pct, threshold
                            ));
                            account.protected_models.insert(lookup_key.clone());
                        }
                    } else {
                        if account.protected_models.contains(&lookup_key) {
                            crate::modules::logger::log_info(&format!(
                                "[Quota] Model protection recovered: {} (Group: {} Max: {}% >= Thres: {}%)",
                                account.email, lookup_key, max_pct, threshold
                            ));
                            account.protected_models.remove(&lookup_key);
                        }
                    }
                }

                // [Compatibility] Migrate from account-level to model-level protection if previously disabled for quota
                if account.proxy_disabled
                    && account
                        .proxy_disabled_reason
                        .as_ref()
                        .map_or(false, |r| r == "quota_protection")
                {
                    crate::modules::logger::log_info(&format!(
                        "[Quota] Migrating account {} from account-level to model-level protection",
                        account.email
                    ));
                    account.proxy_disabled = false;
                    account.proxy_disabled_reason = None;
                    account.proxy_disabled_at = None;
                }
            }
        } else {
            // [FIX] 当配额保护在全局关闭时，清空受保护模型列表，避免遗留历史锁
            if !account.protected_models.is_empty() {
                crate::modules::logger::log_info(&format!(
                    "[Quota] Quota protection disabled globally, clearing protected models for {}",
                    account.email
                ));
                account.protected_models.clear();
            }
        }
    }
    // --- Quota protection logic end ---

    // If quota retrieval succeeded and quota is not forbidden, auto-recover quota_protection locks
    if let Some(ref q) = account.quota {
        if !q.is_forbidden && !account.banned {
            clear_validation_blocked(&mut account, false);
            // 403 Forbidden 与上游封禁绝不因日配额刷新返回 200 而自动解除（Daily 接口对封禁账号仍返回 200）。
            // 只有定向测活成功 (clear_account_forbidden_and_validation) 或用户手动操作才能解封。
            // 仅对配额保护 (quota_protection) 在额度回升后自动解除。
            if account.proxy_disabled
                && account
                    .proxy_disabled_reason
                    .as_deref()
                    .is_some_and(|reason| {
                        classify_lock_reason(reason) == AccountLockKind::QuotaProtection
                    })
            {
                if account.manual_proxy_disabled {
                    account.proxy_disabled = true;
                    account.proxy_disabled_reason = Some("manual".to_string());
                } else {
                    crate::modules::logger::log_info(&format!(
                        "[Quota] Account {} recovered successfully, clearing proxy_disabled",
                        account.email
                    ));
                    account.proxy_disabled = false;
                    account.proxy_disabled_reason = None;
                    account.proxy_disabled_at = None;
                }
            }
        }
    }

    // Quota snapshots may recover before an explicit long image lock expires. Other live
    // records retain the baseline percentage-based cleanup behavior.
    if let Some(ref q) = account.quota {
        let now = chrono::Utc::now().timestamp();
        account.live_limited_models.retain(|model_key, status| {
            if crate::proxy::rate_limit::is_active_persisted_long_image_limit(
                model_key, status, now,
            ) {
                return true;
            }
            let recovered = q.models.iter().any(|model| {
                let is_matching = model.name == *model_key
                    || crate::proxy::common::model_mapping::normalize_to_standard_id(&model.name)
                        .is_some_and(|standard| standard == *model_key);
                is_matching && model.percentage > 0
            });
            !recovered
        });
    }

    // Save account first
    save_account(&account)?;

    // [FIX #3726] 支持可选的索引同步：单次更新时即时同步，批量刷新时延后合并写盘以杜绝磁盘 I/O 放大
    if sync_index {
        if let Ok(mut index) = load_account_index() {
            if let Some(summary) = index.accounts.iter_mut().find(|a| a.id == account_id) {
                summary.protected_models = account.protected_models.clone();
                summary.proxy_disabled = account.proxy_disabled;
                summary.disabled = account.disabled;
                let _ = save_account_index(&index);
            }
        }
    }

    // [FIX] Trigger TokenManager account reload signal
    // This ensures in-memory protected_models are updated
    crate::proxy::server::trigger_account_reload(account_id);

    Ok(account)
}

/// Update account quota (convenience wrapper that syncs index immediately)
pub fn update_account_quota(account_id: &str, quota: QuotaData) -> Result<(), String> {
    update_account_quota_internal(account_id, quota, true).map(|_| ())
}

/// 在持有 ACCOUNT_INDEX_LOCK 全局写锁的保护下更新指定账号索引的 proxy_disabled 状态。
pub fn set_account_index_proxy_disabled(account_id: &str, disabled: bool) -> Result<(), String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
    let mut index = load_account_index()?;
    if let Some(s) = index.accounts.iter_mut().find(|a| a.id == account_id) {
        if s.proxy_disabled != disabled {
            s.proxy_disabled = disabled;
            save_account_index(&index)?;
        }
    }
    Ok(())
}

/// Toggle proxy disabled status for an account
pub fn toggle_proxy_status(
    account_id: &str,
    enable: bool,
    reason: Option<&str>,
) -> Result<(), String> {
    toggle_proxy_status_with_intent(account_id, enable, reason, true)
}

pub fn toggle_proxy_status_with_intent(
    account_id: &str,
    enable: bool,
    reason: Option<&str>,
    manual_intent: bool,
) -> Result<(), String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;

    let mut account = load_account(account_id)?;

    // 封禁账号不能通过手动开关重新入池；必须等权威探测确认解封。
    if enable && account.banned {
        return Err(format!(
            "Account is banned by upstream ({}); submit an appeal before enabling proxy",
            account.banned_code.as_deref().unwrap_or("BANNED")
        ));
    }

    // 手动开关只表达用户意图，不能覆盖系统锁。403/验证锁必须走定向测活或验证恢复，
    // 否则会出现「UI 已启用、调度仍跳过」的状态不一致。
    if enable {
        let now = chrono::Utc::now().timestamp();
        if account.has_active_system_lock(now) {
            return Err(
                "Account has an active 403/validation lock (validation-blocked system lock); use targeted warmup/probe to clear it"
                    .to_string(),
            );
        }
    }

    if manual_intent {
        account.manual_proxy_disabled = !enable;
    }
    account.proxy_disabled = !enable;
    account.proxy_disabled_reason = if !enable {
        reason.map(|s| s.to_string())
    } else {
        None
    };
    account.proxy_disabled_at = if !enable {
        Some(chrono::Utc::now().timestamp())
    } else {
        None
    };

    save_account(&account)?;

    // Also update index summary
    let mut index = load_account_index()?;
    if let Some(summary) = index.accounts.iter_mut().find(|a| a.id == account_id) {
        summary.proxy_disabled = !enable;
        save_account_index(&index)?;
    }

    Ok(())
}

/// Find account ID by email (from index)
pub fn find_account_id_by_email(email: &str) -> Option<String> {
    load_account_index()
        .ok()?
        .accounts
        .into_iter()
        .find(|a| a.email == email)
        .map(|a| a.id)
}

/// 将封禁证据写入账号实体（不负责持久化/索引同步）。
fn apply_ban_state(account: &mut Account, info: &AccountBanInfo, now: i64) {
    account.banned = true;
    account.banned_code = Some(info.code.clone());
    account.banned_reason = Some(info.message.clone());
    account.banned_at = Some(now);
    account.ban_appeal_url = info.appeal_url.clone();

    // 封禁账号必须立即退出反代池；`banned:` 前缀不会被自动恢复逻辑误清理。
    account.proxy_disabled = true;
    account.proxy_disabled_reason = Some(format!("banned: {}", info.code));
    account.proxy_disabled_at = Some(now);

    if let Some(url) = info.appeal_url.clone() {
        account.validation_url = Some(url);
    }

    if let Some(ref mut quota) = account.quota {
        quota.is_forbidden = true;
        quota.forbidden_reason = Some(info.message.clone());
        quota.models.clear();
        quota.quota_groups = None;
    } else {
        account.quota = Some(QuotaData {
            models: Vec::new(),
            last_updated: now,
            subscription_tier: None,
            is_forbidden: true,
            forbidden_reason: Some(info.message.clone()),
            model_forwarding_rules: std::collections::HashMap::new(),
            quota_groups: None,
        });
    }
}

fn forbidden_quota_for_ban(account: &Account, info: &AccountBanInfo) -> QuotaData {
    let mut forbidden = QuotaData::new();
    forbidden.is_forbidden = true;
    forbidden.forbidden_reason = Some(info.message.clone());
    forbidden.subscription_tier = account
        .quota
        .as_ref()
        .and_then(|quota| quota.subscription_tier.clone());
    forbidden
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForbiddenApplyResult {
    /// 账号级 403/封禁：必须从反代池移除。
    AccountLocked,
    /// 模型级 403：保留账号，只冻结对应模型。
    ModelLocked(String),
}

pub fn mark_account_forbidden(
    account_id: &str,
    reason: &str,
) -> Result<ForbiddenApplyResult, String> {
    mark_account_forbidden_with_models(account_id, reason, None, None)
}

pub fn mark_account_forbidden_with_model(
    account_id: &str,
    reason: &str,
    target_model: Option<&str>,
) -> Result<ForbiddenApplyResult, String> {
    mark_account_forbidden_with_models(account_id, reason, target_model, None)
}

pub fn mark_account_forbidden_with_models(
    account_id: &str,
    reason: &str,
    target_model: Option<&str>,
    alt_model: Option<&str>,
) -> Result<ForbiddenApplyResult, String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;

    let mut account = load_account(account_id)?;
    let now = chrono::Utc::now().timestamp();

    // 账号级封禁是最高优先级：已封禁账号不接受模型级 403 的降级覆盖，
    // 保持 banned_code / banned_reason / "banned:" 代理原因不被改写。
    if account.banned {
        crate::modules::logger::log_info(&format!(
            "🛡️ [Ban] Account {} is already banned ({}); ignoring model-level 403 reason",
            account.email,
            account.banned_code.as_deref().unwrap_or("BANNED")
        ));
        return Ok(ForbiddenApplyResult::AccountLocked);
    }

    let ban_info = AccountBanInfo::classify_text(reason);

    // 封禁永远第一优先（banned > validation）：只要命中 TOS 封禁，绝不降级为验证锁！
    if let Some(ref info) = ban_info {
        let action_url = info
            .appeal_url
            .clone()
            .or_else(|| extract_action_url(reason));
        if let Some(url) = action_url {
            account.validation_url = Some(url);
        }
        apply_ban_state(&mut account, info, now);
        crate::modules::logger::log_warn(&format!(
            "🚫 [Ban] Account {} is banned by upstream: {} - {}",
            account.email, info.code, info.message
        ));
        save_account(&account)?;
        if let Ok(mut index) = load_account_index() {
            if let Some(summary) = index.accounts.iter_mut().find(|a| a.id == account_id) {
                summary.proxy_disabled = true;
                let _ = save_account_index(&index);
            }
        }
        crate::proxy::server::trigger_account_reload(account_id);
        crate::modules::log_bridge::emit_accounts_refreshed();
        return Ok(ForbiddenApplyResult::AccountLocked);
    }

    let lock_kind = classify_lock_reason(reason);

    // 1. 在排除封禁后，如果经判定属于验证挑战（VALIDATION_REQUIRED），进入验证阻断流程
    if lock_kind == AccountLockKind::Validation {
        let block_until = now + (10 * 60);
        mark_validation_blocked(&mut account, reason);
        account.validation_blocked_until = Some(block_until);
        account.proxy_disabled = true;

        // 关键守卫：若账号已有 Hard 403 / Banned 原因，必须保留原证据，绝对不能被降级为 validation_required
        let existing_proxy_kind =
            classify_lock_reason(account.proxy_disabled_reason.as_deref().unwrap_or(""));
        if !matches!(
            existing_proxy_kind,
            AccountLockKind::HardForbidden | AccountLockKind::Banned
        ) {
            account.proxy_disabled_reason = Some("validation_required".to_string());
            account.proxy_disabled_at = Some(now);
        }

        save_account(&account)?;
        if let Ok(mut index) = load_account_index() {
            if let Some(summary) = index.accounts.iter_mut().find(|a| a.id == account_id) {
                summary.proxy_disabled = true;
                let _ = save_account_index(&index);
            }
        }
        crate::proxy::server::trigger_account_reload(account_id);
        crate::modules::log_bridge::emit_accounts_refreshed();
        return Ok(ForbiddenApplyResult::AccountLocked);
    }

    // 2. 模型作用域提取：优先从报错文本提取
    let model_scope = forbidden_model_scope(reason).or_else(|| {
        // 关键守卫（审查项 6）：只有当报错文本具备明确的模型访问语义时，才允许利用当前 target_model 辅助提取为模型级冻结！
        // 纯粹的 OAuth 凭据失效、项目级权限不足（如 "The caller does not have permission"）按账号级 403 处理，避免掩盖真实的账号级权限问题。
        let lower = reason.to_lowercase();
        let has_model_access_semantic = lower.contains("model")
            || lower.contains("publisher")
            || lower.contains("not authorized to access")
            || lower.contains("access denied to")
            || lower.contains("not available for this");
        if has_model_access_semantic
            && !lower.contains("quota project")
            && !lower.contains("user location")
            && !lower.contains("location not supported")
            && !lower.contains("location is not supported")
            && !lower.contains("location not allowed")
        {
            target_model.and_then(|m| {
                let cleaned = crate::models::account::clean_model_token(m)?;
                Some(
                    crate::proxy::common::model_mapping::strip_tier_suffix(&cleaned).to_lowercase(),
                )
            })
        } else {
            None
        }
    });

    let already_account_locked = account.quota.as_ref().is_some_and(|q| q.is_forbidden)
        || (account.proxy_disabled
            && matches!(
                classify_lock_reason(account.proxy_disabled_reason.as_deref().unwrap_or("")),
                AccountLockKind::Banned | AccountLockKind::HardForbidden
            ));

    let outcome = if let Some(scope) = model_scope {
        let base_scope =
            crate::proxy::common::model_mapping::strip_tier_suffix(&scope).to_lowercase();
        account.forbidden_models.insert(base_scope.clone());
        // 消除动态改写盲区：同时将映射前目标模型与实际上游模型全部纳入冻结
        if let Some(target) = target_model {
            if let Some(target_cleaned) = crate::models::account::clean_model_token(target) {
                let target_base =
                    crate::proxy::common::model_mapping::strip_tier_suffix(&target_cleaned)
                        .to_lowercase();
                account.forbidden_models.insert(target_base);
            }
        }
        if let Some(alt) = alt_model {
            if let Some(alt_cleaned) = crate::models::account::clean_model_token(alt) {
                let alt_base = crate::proxy::common::model_mapping::strip_tier_suffix(&alt_cleaned)
                    .to_lowercase();
                account.forbidden_models.insert(alt_base);
            }
        }
        crate::modules::logger::log_warn(&format!(
            "🧊 [Forbidden] Account {} model-scoped 403 recorded: {} - {}",
            account.email, base_scope, reason
        ));
        if already_account_locked {
            ForbiddenApplyResult::AccountLocked
        } else {
            ForbiddenApplyResult::ModelLocked(base_scope)
        }
    } else {
        if let Some(url) = extract_action_url(reason) {
            account.validation_url = Some(url);
        }

        // 1. Update quota status
        if let Some(ref mut q) = account.quota {
            q.is_forbidden = true;
            q.forbidden_reason = Some(reason.to_string());
        } else {
            account.quota = Some(crate::models::QuotaData {
                models: Vec::new(),
                last_updated: now,
                subscription_tier: None,
                is_forbidden: true,
                forbidden_reason: Some(reason.to_string()),
                model_forwarding_rules: std::collections::HashMap::new(),
                quota_groups: None,
            });
        }

        // 2. Disable proxy for this account
        account.proxy_disabled = true;
        let formatted_reason = if reason.starts_with("Forbidden (403):") {
            reason.to_string()
        } else {
            format!("Forbidden (403): {}", reason)
        };
        account.proxy_disabled_reason = Some(formatted_reason);
        account.proxy_disabled_at = Some(now);
        ForbiddenApplyResult::AccountLocked
    };

    save_account(&account)?;

    // 3. Update index summary（模型级 403 不把账号标成全局禁用）
    if let Ok(mut index) = load_account_index() {
        if let Some(summary) = index.accounts.iter_mut().find(|a| a.id == account_id) {
            summary.proxy_disabled = account.proxy_disabled;
            if let Err(e) = save_account_index(&index) {
                crate::modules::logger::log_warn(&format!(
                    "Failed to persist index proxy_disabled for {}: {}",
                    account.email, e
                ));
            }
        }
    }

    // 4. 触发账号重载：账号级锁会移除；模型级锁会让 TokenManager 更新 forbidden_models。
    crate::proxy::server::trigger_account_reload(account_id);

    // 5. Notify frontend to refresh account list
    crate::modules::log_bridge::emit_accounts_refreshed();

    Ok(outcome)
}

/// 持久化一次数据面推理探针探测到的封禁状态。
///
/// 返回 `true` 表示状态发生了变化（用于日志/事件抑制）。
pub fn mark_account_banned(account_id: &str, info: &AccountBanInfo) -> Result<bool, String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;

    let mut account = load_account(account_id)?;
    let now = chrono::Utc::now().timestamp();
    let changed = !account.banned
        || account.banned_code.as_deref() != Some(info.code.as_str())
        || account.banned_reason.as_deref() != Some(info.message.as_str())
        || account.ban_appeal_url != info.appeal_url;

    apply_ban_state(&mut account, info, now);
    save_account(&account)?;

    if let Ok(mut index) = load_account_index() {
        if let Some(summary) = index.accounts.iter_mut().find(|a| a.id == account_id) {
            summary.proxy_disabled = true;
            let _ = save_account_index(&index);
        }
    }

    crate::proxy::server::trigger_account_reload(account_id);
    if changed {
        crate::modules::logger::log_warn(&format!(
            "🚫 [Ban] Account {} marked as banned: {} - {}",
            account.email, info.code, info.message
        ));
        crate::modules::log_bridge::emit_accounts_refreshed();
    }

    Ok(changed)
}

/// 清除封禁状态（仅在数据面推理探针确认恢复 2xx 后调用）。
///
/// 只清理由封禁写入的字段；若反代禁用原因是用户手动设置，则保留不动。
pub fn clear_account_ban(account_id: &str) -> Result<bool, String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;

    let mut account = load_account(account_id)?;
    if !account.banned {
        return Ok(false);
    }

    let previous_appeal_url = account.ban_appeal_url.clone();
    account.banned = false;
    account.banned_code = None;
    account.banned_reason = None;
    account.banned_at = None;
    account.ban_appeal_url = None;

    // 仅解除由封禁引起的反代禁用，避免覆盖用户手动禁用。
    if account.manual_proxy_disabled {
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("manual".to_string());
        account.proxy_disabled_at = Some(chrono::Utc::now().timestamp());
    } else if account.proxy_disabled
        && account
            .proxy_disabled_reason
            .as_deref()
            .map_or(false, |reason| reason.starts_with("banned:"))
    {
        account.proxy_disabled = false;
        account.proxy_disabled_reason = None;
        account.proxy_disabled_at = None;
    }

    // 清掉由封禁写入的申诉链接，避免 UI 残留。
    if account.validation_url.is_some() && account.validation_url == previous_appeal_url {
        account.validation_url = None;
    }

    if let Some(ref mut quota) = account.quota {
        quota.is_forbidden = false;
        quota.forbidden_reason = None;
    }

    save_account(&account)?;

    if let Ok(mut index) = load_account_index() {
        if let Some(summary) = index.accounts.iter_mut().find(|a| a.id == account_id) {
            summary.proxy_disabled = account.proxy_disabled;
            let _ = save_account_index(&index);
        }
    }

    crate::proxy::server::trigger_account_reload(account_id);
    crate::modules::logger::log_info(&format!(
        "✅ [Ban] Account {} recovered; ban state cleared",
        account.email
    ));
    crate::modules::log_bridge::emit_accounts_refreshed();

    Ok(true)
}

/// 判断 403/禁用原因是否显式绑定到了与当前测活模型不同的特定模型。
///
/// 若原因为特定模型 403（例如 "User not authorized to access model claude-3-5-sonnet"），
/// 使用其他模型（如 gemini-2.5-flash）探活成功绝不能解除该账号全局 403 锁定。
pub(crate) fn is_forbidden_reason_tied_to_different_model(
    reason: &str,
    tested_model: &str,
) -> bool {
    // 1. 如果原因是人机验证或通用限制，属于账号级，不绑定特定模型
    let lower = reason.to_lowercase();
    if lower.contains("validation_required")
        || lower.contains("challenge")
        || lower.contains("verify your account")
        || lower.contains("user location")
        || lower.contains("location not supported")
        || lower.contains("location is not supported")
        || lower.contains("quota project")
    {
        return false;
    }

    // 2. 统一复用 forbidden_model_scope 提取模型 scope
    if let Some(scope) = forbidden_model_scope(reason) {
        // 若提取出的 scope 能够覆盖当前测活的模型，说明正是针对该模型的测活，非“不同模型”
        if crate::models::account::model_scope_matches(&scope, tested_model) {
            return false;
        }
        return true;
    }

    false
}

/// 判断原因是否带模型级指向。泛化的 `model` 字样也视为模型级，用于保守保留
/// 账号级 403 锁，避免数据面探针 NotBanned 时误清一个仍只影响单个模型的 403。
fn is_model_scoped_forbidden_reason(reason: &str) -> bool {
    let lower = reason.to_lowercase();
    if lower.contains("validation_required")
        || lower.contains("challenge")
        || lower.contains("verify your account")
        || lower.contains("location")
        || lower.contains("quota project")
    {
        return false;
    }
    forbidden_model_scope(reason).is_some() || lower.contains("model")
}

/// 是否存在可由数据面推理探针 `NotBanned` 解除的账号级 403 锁。
///
/// 调用方必须只在 `probe_account_ban` 返回 `NotBanned` 时使用；Daily/Sandbox
/// 配额 200 不是权威证据，绝不能走这里。只要 proxy 或 quota 任一来源带模型级
/// 指向，就保持原锁，等定向测活按模型解除。
fn has_non_model_forbidden_lock(account: &Account) -> bool {
    // 账号级封禁优先级最高：banned 账号的 403/配额状态由 clear_account_ban 统一管理，
    // 不允许 403 清理路径抢先改动。
    if account.banned {
        return false;
    }

    let mut has_hard_lock = false;
    let mut has_model_scope = false;

    if account.proxy_disabled {
        if let Some(reason) = account.proxy_disabled_reason.as_deref() {
            if classify_lock_reason(reason) == AccountLockKind::HardForbidden {
                has_hard_lock = true;
                has_model_scope |= is_model_scoped_forbidden_reason(reason);
            }
        }
    }

    if let Some(quota) = account.quota.as_ref() {
        if quota.is_forbidden {
            if let Some(reason) = quota.forbidden_reason.as_deref() {
                if classify_lock_reason(reason) == AccountLockKind::HardForbidden {
                    has_hard_lock = true;
                    has_model_scope |= is_model_scoped_forbidden_reason(reason);
                }
            } else {
                // 纯 is_forbidden、无原因文本：按账号级 403 处理。
                has_hard_lock = true;
            }
        }
    }

    has_hard_lock && !has_model_scope
}

fn clear_non_model_forbidden_state(account: &mut Account) {
    // 防御性守卫：即使调用方判断失误，也绝不在 banned 状态下清 403。
    if account.banned {
        return;
    }

    if account.manual_proxy_disabled {
        account.proxy_disabled = true;
        account.proxy_disabled_reason = Some("manual".to_string());
        account.proxy_disabled_at = Some(chrono::Utc::now().timestamp());
    } else if account.proxy_disabled
        && account
            .proxy_disabled_reason
            .as_deref()
            .is_some_and(|reason| classify_lock_reason(reason) == AccountLockKind::HardForbidden)
    {
        account.proxy_disabled = false;
        account.proxy_disabled_reason = None;
        account.proxy_disabled_at = None;
    }

    if let Some(quota) = account.quota.as_mut() {
        if quota.is_forbidden {
            quota.is_forbidden = false;
            quota.forbidden_reason = None;
        }
    }
}

/// 判断两个 Gemini 模型是否属于真实动态改写所覆盖的同族变体（仅 Pro 文本、Pro 生图、Flash 生图）。
///
/// 【业务与上游架构契约】：
/// 1. 配额层：Gemini 3.8 及以下的全部文本模型在上游物理机制中均共用同一个 Gemini 通用配额大桶（`bucket_id: "gemini"`），
///    在调度层始终由通用大池统筹调度，不需要人工维护小桶；
/// 2. 状态层：权限冻结与测活解限严格遵循同线通道对齐原则。同一条文本 Pro 线（如 `gemini-3-pro` ↔ `gemini-3.1-pro`）
///    或生图线共享同一套高阶授权，测活任一变体成功即成对解冻自愈，避免用户重复测活；
/// 3. 严格禁止文本 Flash ↔ Pro 互相越权解冻，严格禁止基础文本 Flash 跨代解冻，确保不同授权层级的权限隔离。
fn is_gemini_dynamic_rewrite_pair(a: &str, b: &str) -> bool {
    let a = a.trim().to_lowercase();
    let b = b.trim().to_lowercase();
    if !a.contains("gemini") || !b.contains("gemini") {
        return false;
    }

    let a_is_image = a.contains("image") || a.starts_with("imagen");
    let b_is_image = b.contains("image") || b.starts_with("imagen");
    let a_is_flash = a.contains("flash");
    let b_is_flash = b.contains("flash");
    let a_is_pro = a.contains("pro");
    let b_is_pro = b.contains("pro");

    // 1. 文本 Pro 族：两者均是 pro，且均非 image、非 flash
    let both_text_pro =
        a_is_pro && b_is_pro && !a_is_image && !b_is_image && !a_is_flash && !b_is_flash;
    if both_text_pro {
        return true;
    }

    // 2. Pro 生图族：两者均是 pro 且均是 image
    let both_pro_image = a_is_pro && b_is_pro && a_is_image && b_is_image;
    if both_pro_image {
        return true;
    }

    // 3. Flash 生图族：两者均是 flash 且均是 image
    let both_flash_image = a_is_flash && b_is_flash && a_is_image && b_is_image;
    if both_flash_image {
        return true;
    }

    false
}

/// 测活探测成功后，自动解除账号的 403 封禁及验证锁定状态，恢复其在反代池中的调度资格。
///
/// 边界防护契约：
/// 1. 严格拦截 `account.banned`（TOS 违规等封禁绝不自动解封，保留申诉 URL 与封禁标识）；
/// 2. 显式清除 `quota.is_forbidden` 与理由，即使账号无 validation/proxy_disabled 也彻底解封；
/// 3. 若 403 明确绑定了某个具体模型（例如 Claude 专属 403），而测活模型与之不匹配，则不越权清除账号级 403。
pub fn clear_account_forbidden_and_validation(
    account_id: &str,
    tested_model: Option<&str>,
) -> Result<bool, String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;

    let mut account = load_account(account_id)?;

    // 契约 1：TOS 等真正封禁账号绝不被探针流程篡改解封
    if account.banned {
        return Ok(false);
    }

    let had_forbidden = account.quota.as_ref().is_some_and(|q| q.is_forbidden);
    let had_validation = account.validation_blocked
        || account.validation_blocked_until.is_some()
        || account.validation_blocked_reason.is_some()
        || account.validation_url.is_some();
    let had_proxy_disabled = account.proxy_disabled
        && account
            .proxy_disabled_reason
            .as_deref()
            .is_some_and(|reason| {
                matches!(
                    classify_lock_reason(reason),
                    AccountLockKind::HardForbidden | AccountLockKind::Validation
                )
            });
    let had_model_forbidden = !account.forbidden_models.is_empty();

    if !had_forbidden && !had_validation && !had_proxy_disabled && !had_model_forbidden {
        return Ok(false);
    }

    // 步骤 1：模型级 403 解冻。优先解开与实测模型匹配的 forbidden_models scope。
    // 全量探活（tested_model == None）未验证特定第三方模型，不粗暴清空 forbidden_models。
    let mut model_cleared = false;
    if had_model_forbidden {
        if let Some(tested) = tested_model {
            let before = account.forbidden_models.len();
            let tested_lower = tested.trim().to_lowercase();
            let tested_base = crate::proxy::common::model_mapping::strip_tier_suffix(&tested_lower)
                .to_lowercase();
            let tested_img_base =
                crate::proxy::common::model_mapping::strip_image_variant_suffix(&tested_lower)
                    .to_lowercase();

            account.forbidden_models.retain(|scope| {
                let s_lower = scope.trim().to_lowercase();
                // 1. 基础单向匹配（涵盖自身及档位变体）
                if crate::models::account::model_scope_matches(&s_lower, &tested_lower)
                    || crate::models::account::model_scope_matches(&s_lower, &tested_base)
                {
                    return false; // 清除
                }
                // 2. 生图尺寸变体对齐（如 -4k/-2k/-preview）
                if crate::models::account::model_scope_matches(&s_lower, &tested_img_base) {
                    return false; // 清除
                }
                // 3. 严格限定同类动态改写通道内的成对解冻（审查项 P1）：
                //    仅允许文本 Pro ↔ 文本 Pro、Pro 生图 ↔ Pro 生图、Flash 生图 ↔ Flash 生图；
                //    坚决禁止文本 Flash ↔ Pro 互相误解冻，坚决禁止基础文本 Flash 跨代误解冻！
                if is_gemini_dynamic_rewrite_pair(&s_lower, &tested_lower) {
                    return false;
                }
                true // 保留其他模型
            });
            model_cleared = account.forbidden_models.len() != before;
        }
    }

    // 步骤 2：账号级 403 / 验证锁自愈检查。
    // 契约 3：若任一账号级 403 原因显式绑定了不同模型，不越权清除账号级状态。
    let mut allow_account_level_unblock = true;
    if let Some(tested) = tested_model {
        let reason_candidates = [
            account.proxy_disabled_reason.as_deref(),
            account
                .quota
                .as_ref()
                .and_then(|q| q.forbidden_reason.as_deref()),
        ];
        for reason in reason_candidates.into_iter().flatten() {
            if is_forbidden_reason_tied_to_different_model(reason, tested) {
                crate::modules::logger::log_warn(&format!(
                    "⚠️ [Probe] Account {} probe succeeded for {}, but account 403 lock is tied to a different model ({}); keeping account-level lock",
                    account.email, tested, reason
                ));
                allow_account_level_unblock = false;
                break;
            }
        }
    }

    let mut changed = model_cleared;

    // 步骤 3：验证锁自愈（测活成功是上游真实调用成功的权威证据，证明人机验证已通过）
    if had_validation {
        clear_validation_blocked(&mut account, true);
        changed = true;
    }

    // 步骤 4：账号级 403 解锁裁决
    if allow_account_level_unblock && had_forbidden {
        if let Some(ref mut q) = account.quota {
            q.is_forbidden = false;
            q.forbidden_reason = None;
        }
        changed = true;
    }

    if had_proxy_disabled {
        if account.manual_proxy_disabled {
            account.proxy_disabled = true;
            account.proxy_disabled_reason = Some("manual".to_string());
        } else {
            let proxy_reason = account.proxy_disabled_reason.as_deref().unwrap_or("");
            let proxy_kind = classify_lock_reason(proxy_reason);

            match proxy_kind {
                AccountLockKind::Validation => {
                    // 关键修复（审查项 2）：若 proxy 停用原因本身是验证阻断，测活成功已证实人机验证通过，
                    // 必须立即清除验证类型的 proxy 锁，绝不能被其他异构模型的 quota 403 阻碍残留！
                    account.proxy_disabled = false;
                    account.proxy_disabled_reason = None;
                    account.proxy_disabled_at = None;
                    changed = true;
                }
                AccountLockKind::HardForbidden => {
                    // 仅当账号级 403 允许解封（原因未绑定不同模型）时，才解除 HardForbidden proxy 锁
                    if allow_account_level_unblock {
                        account.proxy_disabled = false;
                        account.proxy_disabled_reason = None;
                        account.proxy_disabled_at = None;
                        changed = true;
                    }
                }
                _ => {}
            }
        }
    }

    if !changed {
        return Ok(false);
    }

    save_account(&account)?;

    if let Ok(mut index) = load_account_index() {
        if let Some(summary) = index.accounts.iter_mut().find(|a| a.id == account_id) {
            summary.proxy_disabled = account.proxy_disabled;
            let _ = save_account_index(&index);
        }
    }

    crate::proxy::server::trigger_account_reload(account_id);
    crate::modules::logger::log_info(&format!(
        "✅ [Probe] Account {} verified live and functional; cleared forbidden/validation locks",
        account.email
    ));
    crate::modules::log_bridge::emit_accounts_refreshed();

    Ok(true)
}

pub fn mark_account_validation_blocked(
    account_id: &str,
    block_until: i64,
    reason: &str,
) -> Result<(), String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;

    let mut account = load_account(account_id)?;

    mark_validation_blocked(&mut account, reason);
    account.validation_blocked_until = Some(block_until);

    save_account(&account)?;

    // Update index summary to keep state synchronized
    let mut index = load_account_index()?;
    if let Some(_summary) = index.accounts.iter_mut().find(|a| a.id == account_id) {
        save_account_index(&index)?;
    }

    crate::modules::log_bridge::emit_accounts_refreshed();
    Ok(())
}

/// Export accounts by IDs (for backup/migration)
pub fn export_accounts_by_ids(
    account_ids: &[String],
) -> Result<crate::models::AccountExportResponse, String> {
    use crate::models::{AccountExportItem, AccountExportResponse};

    let target_ids: Vec<String> = if account_ids.is_empty() {
        let index = load_account_index()?;
        index.accounts.into_iter().map(|a| a.id).collect()
    } else {
        account_ids.to_vec()
    };

    let mut export_items: Vec<AccountExportItem> = Vec::with_capacity(target_ids.len());
    for id in &target_ids {
        if let Ok(acc) = load_account(id) {
            export_items.push(AccountExportItem {
                email: acc.email,
                refresh_token: acc.token.refresh_token,
            });
        }
    }

    Ok(AccountExportResponse {
        accounts: export_items,
    })
}

/// Export all accounts' refresh_tokens (legacy, kept for compatibility)
#[allow(dead_code)]
pub fn export_accounts() -> Result<Vec<(String, String)>, String> {
    let accounts = list_accounts()?;
    let mut exports = Vec::new();

    for account in accounts {
        exports.push((account.email, account.token.refresh_token));
    }

    Ok(exports)
}

/// 将模型级 403 的配额快照降级为模型冻结，避免返回给 UI/调用方的
/// `is_forbidden` 把整号显示成封禁。
fn demote_model_scoped_quota(account: &mut Account, mut quota: QuotaData) -> QuotaData {
    if account.banned {
        return quota;
    }
    if let Some(scope) = quota
        .forbidden_reason
        .as_deref()
        .and_then(forbidden_model_scope)
    {
        account.forbidden_models.insert(scope);
        quota.is_forbidden = false;
        quota.forbidden_reason = None;
        // 模型级 403 快照通常没有 models；保留上一次成功刷新得到的其他模型配额，
        // 避免本次 API 响应把整页配额显示成空。
        if quota.models.is_empty() {
            if let Some(existing) = &account.quota {
                quota.models = existing.models.clone();
                if quota.quota_groups.is_none() {
                    quota.quota_groups = existing.quota_groups.clone();
                }
            }
        }
    }
    quota
}

/// Quota query with retry (moved from commands to modules for reuse)
pub async fn fetch_quota_with_retry(account: &mut Account) -> crate::error::AppResult<QuotaData> {
    fetch_quota_with_retry_ext(account, false).await
}

/// 强制进行权威数据面封禁探针的配额刷新（供单账号手动刷新、添加新账号、批量导入等显式用户意图使用）
pub async fn fetch_quota_with_retry_forced(
    account: &mut Account,
) -> crate::error::AppResult<QuotaData> {
    fetch_quota_with_retry_ext(account, true).await
}

pub async fn fetch_quota_with_retry_ext(
    account: &mut Account,
    force_probe: bool,
) -> crate::error::AppResult<QuotaData> {
    use crate::error::AppError;
    use crate::modules::oauth;

    // 1. Time-based check - ensure Token is valid first
    let token = match oauth::ensure_fresh_token(&account.token, Some(&account.id)).await {
        Ok(t) => t,
        Err(e) => {
            if e.contains("invalid_grant") {
                modules::logger::log_error(&format!(
                    "Disabling account {} due to invalid_grant during token refresh (quota check)",
                    account.email
                ));
                account.disabled = true;
                account.disabled_at = Some(chrono::Utc::now().timestamp());
                account.disabled_reason = Some(format!("invalid_grant: {}", e));
                let _ = save_account(account);
                crate::proxy::server::trigger_account_reload(&account.id);
            }
            return Err(AppError::OAuth(e));
        }
    };

    if token.access_token != account.token.access_token {
        modules::logger::log_info(&format!("Time-based Token refresh: {}", account.email));
        account.token = token.clone();

        // Get display name (incidental to Token refresh)
        let name = if account.name.is_none()
            || account.name.as_ref().map_or(false, |n| n.trim().is_empty())
        {
            match oauth::get_user_info(&token.access_token, Some(&account.id)).await {
                Ok(user_info) => user_info.get_display_name(),
                Err(_) => None,
            }
        } else {
            account.name.clone()
        };

        account.name = name.clone();
        upsert_account(account.email.clone(), name, token.clone()).map_err(AppError::Account)?;
    }

    // 0. Supplement display name (if missing or upper step failed)
    if account.name.is_none() || account.name.as_ref().map_or(false, |n| n.trim().is_empty()) {
        modules::logger::log_info(&format!(
            "Account {} missing display name, attempting to fetch...",
            account.email
        ));
        // Use updated token
        match oauth::get_user_info(&account.token.access_token, Some(&account.id)).await {
            Ok(user_info) => {
                let display_name = user_info.get_display_name();
                modules::logger::log_info(&format!(
                    "Successfully fetched display name: {:?}",
                    display_name
                ));
                account.name = display_name.clone();
                // Save immediately
                if let Err(e) =
                    upsert_account(account.email.clone(), display_name, account.token.clone())
                {
                    modules::logger::log_warn(&format!("Failed to save display name: {}", e));
                }
            }
            Err(e) => {
                modules::logger::log_warn(&format!("Failed to fetch display name: {}", e));
            }
        }
    }

    // 1.4 为数据面探针补齐 project 上下文。
    // 控制面 loadCodeAssist 对被封禁账号同样返回 200，因此它只用于提供 project，
    // 绝不能作为封禁判据。
    let probe_project_id = match account
        .token
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|pid| !pid.is_empty())
    {
        Some(pid) => Some(pid.to_string()),
        None => {
            crate::modules::quota::resolve_project_id(
                &account.token.access_token,
                &account.email,
                Some(&account.id),
            )
            .await
        }
    };

    // 1.5 数据面权威封禁探针（长时智能冷却）：
    // - 强制探测（force_probe = true）：单账号手动刷新、添加新号、批量导入时无条件执行；
    // - 已封禁账号（account.banned）：冷却 1 小时（3600 秒），防止后台定时刷新每几分钟去轰炸 Google 导致 429；
    // - 正常健康账号（!account.banned）：日常后台自动定时刷新只拉轻量配额接口（retrieveUserQuotaSummary），
    //   探针至少间隔 6 小时（21600 秒）才做一次常规确认。运行时若遭遇封禁，实际请求会当场返回 403 触发禁用，绝不漏判！
    let now = chrono::Utc::now().timestamp();
    let should_probe = if force_probe {
        true
    } else if account.banned {
        account
            .last_ban_probed_at
            .map_or(true, |ts| now.saturating_sub(ts) >= 3600)
    } else {
        account
            .last_ban_probed_at
            .map_or(true, |ts| now.saturating_sub(ts) >= 21600)
    };

    if should_probe {
        account.last_ban_probed_at = Some(now);

        match crate::modules::quota::probe_account_ban(
            &account.token.access_token,
            probe_project_id.as_deref(),
            Some(&account.id),
            Some(&account.email),
        )
        .await
        {
            crate::modules::quota::BanProbeOutcome::Banned(info) => {
                apply_ban_state(account, &info, now);

                if let Err(e) = mark_account_banned(&account.id, &info) {
                    modules::logger::log_warn(&format!(
                        "Failed to persist ban state for {}: {}",
                        account.email, e
                    ));
                }

                return Ok(forbidden_quota_for_ban(account, &info));
            }
            crate::modules::quota::BanProbeOutcome::NotBanned => {
                if account.banned {
                    match clear_account_ban(&account.id) {
                        Ok(_) => {
                            account.banned = false;
                            account.banned_code = None;
                            account.banned_reason = None;
                            account.banned_at = None;
                            account.ban_appeal_url = None;
                            if account
                                .proxy_disabled_reason
                                .as_deref()
                                .map_or(false, |reason| reason.starts_with("banned:"))
                            {
                                account.proxy_disabled = false;
                                account.proxy_disabled_reason = None;
                                account.proxy_disabled_at = None;
                            }
                            if let Some(ref mut quota) = account.quota {
                                quota.is_forbidden = false;
                                quota.forbidden_reason = None;
                            }
                        }
                        Err(e) => modules::logger::log_warn(&format!(
                            "Failed to clear ban state for {}: {}",
                            account.email, e
                        )),
                    }
                }

                // 数据面推理探针 2xx 是账号级封禁/403 的权威恢复证据。
                // 普通配额 200 不会进入这个分支，因此拉额度无法欺骗解封；
                // 模型级 403 由 `has_non_model_forbidden_lock` 保守排除，
                // 仍需定向测活按模型解除。
                if has_non_model_forbidden_lock(account) {
                    clear_non_model_forbidden_state(account);
                    if let Err(e) = save_account(account) {
                        modules::logger::log_warn(&format!(
                            "Failed to persist recovered 403 state for {}: {}",
                            account.email, e
                        ));
                    }
                    crate::proxy::server::trigger_account_reload(&account.id);
                    crate::modules::log_bridge::emit_accounts_refreshed();
                    modules::logger::log_info(&format!(
                        "✅ [Probe] Account {} recovered from account-level 403 lock",
                        account.email
                    ));
                }
            }
            crate::modules::quota::BanProbeOutcome::Unknown => {
                // 探针不可用（网络/429/5xx/非封禁 403）时保留现状，避免把仍被封禁的账号误放回池。
                // 已封禁账号：探针未返回权威 2xx，坚决阻断日配额接口，绝不被配额 200 反向洗白！
                if account.banned {
                    let info =
                        account
                            .ban_info()
                            .unwrap_or_else(|| crate::models::AccountBanInfo {
                                code: "BANNED".to_string(),
                                message: "Account remains banned".to_string(),
                                appeal_url: account.ban_appeal_url.clone(),
                            });
                    return Ok(forbidden_quota_for_ban(account, &info));
                }

                // 兼容旧版本已经写入 "Forbidden (403): ..." / TOS 文本、但还没有新增
                // `banned` 字段的账号：只要持久化原因里仍带明确封禁证据，就升级为
                // banned 状态，避免随后 Daily 配额 200 把这条封禁记录清掉。
                let legacy_info = {
                    let reason = account.proxy_disabled_reason.as_deref().or_else(|| {
                        account
                            .quota
                            .as_ref()
                            .and_then(|q| q.forbidden_reason.as_deref())
                    });
                    reason.and_then(crate::models::AccountBanInfo::classify_text)
                };

                if let Some(info) = legacy_info {
                    apply_ban_state(account, &info, now);
                    if let Err(e) = mark_account_banned(&account.id, &info) {
                        modules::logger::log_warn(&format!(
                            "Failed to upgrade legacy ban state for {}: {}",
                            account.email, e
                        ));
                    }
                    return Ok(forbidden_quota_for_ban(account, &info));
                }

                // 既有 Hard 403 锁定的账号同理：探针未知时绝不调用日配额端点，防止被 200 冲刷。
                if let Some(ref q) = account.quota {
                    if q.is_forbidden {
                        let kind = crate::models::account::classify_lock_reason(
                            q.forbidden_reason.as_deref().unwrap_or(""),
                        );
                        if kind == crate::models::account::AccountLockKind::HardForbidden {
                            return Ok(q.clone());
                        }
                    }
                }
            }
        }
    } else if account.banned {
        // 在长时冷却期内且已被封禁的账号：直接返回封禁配额，跳过无意义的配额与探针请求
        let info = account
            .ban_info()
            .unwrap_or_else(|| crate::models::AccountBanInfo {
                code: "BANNED".to_string(),
                message: "Account remains banned".to_string(),
                appeal_url: account.ban_appeal_url.clone(),
            });
        return Ok(forbidden_quota_for_ban(account, &info));
    }

    // 2. 仅在探针未判定封禁后拉取配额。
    let result: crate::error::AppResult<(QuotaData, Option<String>)> =
        modules::fetch_quota_with_cache(
            &account.token.access_token,
            &account.email,
            probe_project_id.as_deref(),
            Some(&account.id),
        )
        .await;

    // Capture potentially updated project_id and save
    if let Ok((ref _q, ref project_id)) = result {
        if project_id.is_some() && *project_id != account.token.project_id {
            modules::logger::log_info(&format!(
                "Detected project_id update ({}), saving...",
                account.email
            ));
            account.token.project_id = project_id.clone();
            if let Err(e) = upsert_account(
                account.email.clone(),
                account.name.clone(),
                account.token.clone(),
            ) {
                modules::logger::log_warn(&format!("Failed to sync project_id: {}", e));
            }
        }
    }

    // 3. Handle 401 error
    if let Err(AppError::Network(_, status)) = result {
        if let Some(code) = status {
            if code == 401 {
                modules::logger::log_warn(&format!(
                    "401 Unauthorized for {}, forcing refresh...",
                    account.email
                ));

                // Force refresh
                let token_res = match oauth::refresh_access_token_with_client(
                    &account.token.refresh_token,
                    Some(&account.id),
                    account.token.oauth_client_key.as_deref(),
                )
                .await
                {
                    Ok(t) => t,
                    Err(e) => {
                        if e.contains("invalid_grant") {
                            modules::logger::log_error(&format!(
                                "Disabling account {} due to invalid_grant during forced refresh (quota check)",
                                account.email
                            ));
                            account.disabled = true;
                            account.disabled_at = Some(chrono::Utc::now().timestamp());
                            account.disabled_reason = Some(format!("invalid_grant: {}", e));
                            let _ = save_account(account);
                            crate::proxy::server::trigger_account_reload(&account.id);
                        }
                        return Err(AppError::OAuth(e));
                    }
                };

                let is_gcp_tos = if crate::modules::oauth::is_gcp_enterprise_client(
                    token_res
                        .oauth_client_key
                        .as_deref()
                        .or(account.token.oauth_client_key.as_deref()),
                ) {
                    true
                } else {
                    account.token.is_gcp_tos
                };

                let new_token = TokenData::new(
                    token_res.access_token.clone(),
                    account.token.refresh_token.clone(),
                    token_res.expires_in,
                    account.token.email.clone(),
                    account.token.project_id.clone(), // Keep original project_id
                    None,                             // Add None as session_id
                    is_gcp_tos,
                    token_res.id_token.clone(),
                )
                .with_oauth_client_key(
                    token_res
                        .oauth_client_key
                        .clone()
                        .or_else(|| account.token.oauth_client_key.clone()),
                );

                // Re-fetch display name
                let name = if account.name.is_none()
                    || account.name.as_ref().map_or(false, |n| n.trim().is_empty())
                {
                    match oauth::get_user_info(&token_res.access_token, Some(&account.id)).await {
                        Ok(user_info) => user_info.get_display_name(),
                        Err(_) => None,
                    }
                } else {
                    account.name.clone()
                };

                account.token = new_token.clone();
                account.name = name.clone();
                upsert_account(account.email.clone(), name, new_token.clone())
                    .map_err(AppError::Account)?;

                // Retry query (pass cached project_id if available)
                let retry_result: crate::error::AppResult<(QuotaData, Option<String>)> =
                    modules::fetch_quota_with_cache(
                        &new_token.access_token,
                        &account.email,
                        account.token.project_id.as_deref(),
                        Some(&account.id),
                    )
                    .await;

                // Also handle project_id saving during retry
                if let Ok((ref _q, ref project_id)) = retry_result {
                    if project_id.is_some() && *project_id != account.token.project_id {
                        modules::logger::log_info(&format!(
                            "Detected update of project_id after retry ({}), saving...",
                            account.email
                        ));
                        account.token.project_id = project_id.clone();
                        let _ = upsert_account(
                            account.email.clone(),
                            account.name.clone(),
                            account.token.clone(),
                        );
                    }
                }

                if let Err(AppError::Network(_, status)) = retry_result {
                    if let Some(code) = status {
                        if code == 403 {
                            let mut q = QuotaData::new();
                            q.is_forbidden = true;
                            // 补全明确的 forbidden_reason，避免落盘后被 TokenManager 误当空原因静默移除
                            q.forbidden_reason =
                                Some("Forbidden (403): Quota query denied".to_string());
                            return Ok(q);
                        }
                    }
                }

                match retry_result {
                    Ok((q, _)) => {
                        let q = demote_model_scoped_quota(account, q);
                        if !q.is_forbidden {
                            clear_validation_blocked(account, false);
                        }
                        return Ok(q);
                    }
                    Err(e) => {
                        if is_validation_required_error(&e) {
                            mark_validation_blocked(account, &e.to_string());
                        }
                        if let Some(cached) = recover_cached_quota_on_rate_limit(account, &e) {
                            mark_validation_blocked(account, &format_rate_limit_block_reason(&e));
                            modules::logger::log_warn(&format!(
                                "Quota API rate-limited for {}, using cached model list as fallback",
                                account.email
                            ));
                            return Ok(cached);
                        }
                        return Err(e);
                    }
                }
            }
        }
    }

    // fetch_quota already handles 403, with additional local fallback/validation handling.
    match result {
        Ok((q, _)) => {
            let q = demote_model_scoped_quota(account, q);
            if !q.is_forbidden {
                clear_validation_blocked(account, false);
            }
            Ok(q)
        }
        Err(e) => {
            if is_validation_required_error(&e) {
                mark_validation_blocked(account, &e.to_string());
            }
            if let Some(cached) = recover_cached_quota_on_rate_limit(account, &e) {
                mark_validation_blocked(account, &format_rate_limit_block_reason(&e));
                modules::logger::log_warn(&format!(
                    "Quota API rate-limited for {}, using cached model list as fallback",
                    account.email
                ));
                return Ok(cached);
            }
            Err(e)
        }
    }
}

#[derive(Serialize)]
pub struct RefreshStats {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub details: Vec<String>,
}

/// Core logic to batch refresh all account quotas (decoupled from Tauri status)
pub async fn refresh_all_quotas_logic() -> Result<RefreshStats, String> {
    use futures::stream::{self, StreamExt};
    let start = std::time::Instant::now();

    let app_cfg = crate::modules::config::load_app_config().ok();
    let max_concurrent = app_cfg
        .as_ref()
        .map(|c| c.refresh_concurrency)
        .unwrap_or(5)
        .clamp(1, 20) as usize;

    let slide_delay_secs = app_cfg
        .as_ref()
        .map(|c| c.refresh_slide_delay_secs)
        .unwrap_or(0.2);
    let slide_delay_secs = if slide_delay_secs.is_finite() {
        slide_delay_secs.clamp(0.0, 10.0)
    } else {
        0.2
    };
    let slide_delay = std::time::Duration::from_secs_f64(slide_delay_secs);

    crate::modules::logger::log_info(&format!(
        "Starting batch refresh of all account quotas (sliding window: max {} concurrent, slot handoff delay {:.2}s)",
        max_concurrent, slide_delay_secs
    ));
    let accounts = list_accounts()?;

    let filtered_accounts: Vec<_> = accounts
        .into_iter()
        .filter(|account| {
            // [MOD] Now we allow refreshing disabled and proxy_disabled accounts
            // to support forced re-sync from UI.
            //
            // 只跳过「验证封禁仍在生效期内」的账号。封禁过期后必须放行刷新 —— 刷新
            // 正是判断"还封不封"的手段；跳过它会让 quota.is_forbidden 永远是 true：
            // 403 把它置 true、只有刷新成功才会写回 false，而刷新又在这里被跳过，
            // 形成环形依赖，账号再也回不来（issue #3630）。
            if let Some(ref q) = account.quota {
                if q.is_forbidden {
                    let now = chrono::Utc::now().timestamp();
                    // 统一走 is_validation_block_active 判定有效冷静期
                    if account.is_validation_block_active(now) {
                        crate::modules::logger::log_info(&format!(
                            "  - Skipping {} (Forbidden, 验证封禁中)",
                            account.email
                        ));
                        return false;
                    }
                    crate::modules::logger::log_info(&format!(
                        "  - Retrying {} (Forbidden 但验证封禁已过期，重新探测)",
                        account.email
                    ));
                }
            }
            true
        })
        .collect();

    let total = filtered_accounts.len();

    let mut success = 0;
    let mut failed = 0;
    let mut details = Vec::new();
    let mut updated_accounts = Vec::new();

    // 滑动窗口：任一账号完成后等待配置的滑动延迟，再释放槽位给下一个账号；
    // 不等待同批其他账号完成，也不会让任何账号等待整批屏障。
    let mut stream = stream::iter(filtered_accounts)
        .map(|mut account| async move {
            let email = account.email.clone();
            let account_id = account.id.clone();
            crate::modules::logger::log_info(&format!("  - Processing {}", email));

            let result = match fetch_quota_with_retry(&mut account).await {
                Ok(quota) => {
                    // 批量模式下不在此处反复重写全局大索引 accounts.json (sync_index: false)
                    match update_account_quota_internal(&account_id, quota, false) {
                        Ok(updated_acc) => {
                            crate::modules::logger::log_info(&format!("    Success {}", email));
                            Ok(updated_acc)
                        }
                        Err(e) => {
                            let msg = format!("Account {}: Save quota failed - {}", email, e);
                            crate::modules::logger::log_error(&msg);
                            Err(msg)
                        }
                    }
                }
                Err(e) => {
                    let msg = format!("Account {}: Fetch quota failed - {}", email, e);
                    crate::modules::logger::log_error(&msg);
                    Err(msg)
                }
            };

            // 实时通知前端流式更新该账号状态（前端采用静默防抖更新，不闪烁、不弹窗、不全屏转圈）
            crate::modules::log_bridge::emit_accounts_refreshed();

            // 滑动窗口：账号完成后等待配置的延迟，再把并发槽位让给下一个账号。
            if !slide_delay.is_zero() {
                tokio::time::sleep(slide_delay).await;
            }
            result
        })
        .buffer_unordered(max_concurrent);

    while let Some(res) = stream.next().await {
        match res {
            Ok(acc) => {
                success += 1;
                updated_accounts.push(acc);
            }
            Err(msg) => {
                failed += 1;
                details.push(msg);
            }
        }
    }

    // [FIX #3726] 批量刷新全部执行完毕后，统一一次性更新 accounts.json 大索引！(I/O 骤减 99.8%)
    if !updated_accounts.is_empty() {
        if let Ok(mut index) = load_account_index() {
            let mut dirty = false;
            for acc in updated_accounts {
                if let Some(summary) = index.accounts.iter_mut().find(|a| a.id == acc.id) {
                    summary.protected_models = acc.protected_models;
                    summary.proxy_disabled = acc.proxy_disabled;
                    summary.disabled = acc.disabled;
                    dirty = true;
                }
            }
            if dirty {
                let _ = save_account_index(&index);
            }
        }
    }

    let elapsed = start.elapsed();
    crate::modules::logger::log_info(&format!(
        "Batch refresh completed: {} success, {} failed, took: {}ms",
        success,
        failed,
        elapsed.as_millis()
    ));

    // After quota refresh, immediately check and trigger warmup for weekly recovered models
    tokio::spawn(async {
        check_and_trigger_warmup_for_recovered_models().await;
    });

    Ok(RefreshStats {
        total,
        success,
        failed,
        details,
    })
}

/// Check and trigger warmup for models that have recovered to 100%
/// Called automatically after quota refresh to enable immediate warmup
pub async fn check_and_trigger_warmup_for_recovered_models() {
    let accounts = match list_accounts() {
        Ok(acc) => acc,
        Err(_) => return,
    };

    // Load config to check if scheduled warmup is enabled
    let app_config = match crate::modules::config::load_app_config() {
        Ok(cfg) => cfg,
        Err(_) => return,
    };

    if !app_config.scheduled_warmup.enabled {
        return;
    }

    crate::modules::logger::log_info(&format!(
        "[Warmup] Checking {} accounts for recovered models after quota refresh...",
        accounts.len()
    ));

    for account in accounts {
        // Skip disabled accounts
        if account.disabled || account.proxy_disabled || account.banned {
            continue;
        }

        // Trigger warmup check for this account
        crate::modules::scheduler::trigger_warmup_for_account(&account).await;
    }
}

/// [NEW #3726] 通用账号凭据提取解析器：兼容各类历史版本格式
/// 支持：
/// 1. 标准导入导出数组：[{ "email": "...", "refresh_token": "..." }]
/// 2. 包含完整账号的实体数组：[{ "id": ..., "email": ..., "token": { "refresh_token": ... } }]
/// 3. 包裹对象结构：{ "accounts": [...] }
/// 4. 字典映射结构：{ "accounts": { "uuid": { "email": ..., "refresh_token": ... } } }
pub fn parse_import_entries_from_str(content: &str) -> Vec<crate::models::ImportAccountEntry> {
    let mut entries = Vec::new();

    if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
        let mut raw_items = Vec::new();

        if let Some(arr) = val.as_array() {
            raw_items.extend(arr.iter().cloned());
        } else if let Some(obj) = val.as_object() {
            if let Some(arr) = obj.get("accounts").and_then(|v| v.as_array()) {
                raw_items.extend(arr.iter().cloned());
            } else if let Some(map) = obj.get("accounts").and_then(|v| v.as_object()) {
                raw_items.extend(map.values().cloned());
            } else {
                raw_items.extend(obj.values().cloned());
            }
        }

        for item in raw_items {
            let email = item
                .get("email")
                .and_then(|v| v.as_str())
                .or_else(|| item.get("name").and_then(|v| v.as_str()))
                .map(|s| s.trim().to_string());

            let rt = item
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    item.get("token")
                        .and_then(|t| t.get("refresh_token"))
                        .and_then(|v| v.as_str())
                })
                .map(|s| s.trim().to_string());

            if let Some(refresh_token) = rt {
                if !refresh_token.is_empty() && refresh_token.starts_with("1//") {
                    entries.push(crate::models::ImportAccountEntry {
                        email,
                        refresh_token,
                    });
                }
            }
        }
    }

    entries
}

/// [NEW #3726] 批量异步高性能导入账号
///
/// 核心工业级优化：
/// 1. 采用 futures::stream::buffer_unordered 严格限制在途并发度为 6，消灭 join_all 任务堆积与内存雪崩；
/// 2. 内存预查重：启动前单次读取索引，基于已存在账号集合秒级过滤，避免重复向 Google 发起网络握手；
/// 3. 单账号独立并行写盘（accounts/{id}.json），过程中绝不争抢或反复重写全局大索引 accounts.json；
/// 4. 全部批次完成时，仅获取一次锁，执行单次原子写盘合并 accounts.json，写盘 I/O 减少 99.8%；
/// 5. 过程中通过 app.emit 向前端流式下发 accounts://import-progress 进度事件，杜绝前端 DOM 列表重绘。
pub async fn batch_import_accounts_logic(
    app: Option<&tauri::AppHandle>,
    entries: Vec<crate::models::ImportAccountEntry>,
) -> Result<crate::models::BatchImportResult, String> {
    use futures::stream::{self, StreamExt};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tauri::Emitter;
    use tokio::sync::Mutex;

    let total = entries.len();
    if total == 0 {
        return Ok(crate::models::BatchImportResult {
            total: 0,
            success: 0,
            failed: 0,
            errors: Vec::new(),
        });
    }

    crate::modules::logger::log_info(&format!(
        "🚀 [Batch Import] Starting high-performance batch import for {} accounts (concurrency: 6)",
        total
    ));

    // 1. 内存预查重：单次加载现存索引，构建已有 Email 与 Refresh Token 到 ID 的映射（用于幂等原地复用）
    let (existing_email_map, existing_rt_map) = {
        let index = load_account_index().unwrap_or_default();
        let mut email_map = std::collections::HashMap::new();
        let mut rt_map = std::collections::HashMap::new();
        for s in &index.accounts {
            email_map.insert(s.email.trim().to_lowercase(), s.id.clone());
            if let Ok(acc) = load_account(&s.id) {
                let rt_clean = acc.token.refresh_token.trim().to_string();
                if !rt_clean.is_empty() {
                    rt_map.insert(rt_clean, s.id.clone());
                }
            }
        }
        (Arc::new(email_map), Arc::new(rt_map))
    };

    const CONCURRENCY: usize = 6;
    let processed_counter = Arc::new(AtomicUsize::new(0));
    let success_counter = Arc::new(AtomicUsize::new(0));
    let failed_counter = Arc::new(AtomicUsize::new(0));
    let errors_list = Arc::new(Mutex::new(Vec::new()));

    // 2. 流式异步并发消费，内存常驻活跃任务 <= CONCURRENCY
    let stream = stream::iter(entries)
        .map(|entry| {
            let existing_email_map = existing_email_map.clone();
            let existing_rt_map = existing_rt_map.clone();
            let processed_counter = processed_counter.clone();
            let success_counter = success_counter.clone();
            let failed_counter = failed_counter.clone();
            let errors_list = errors_list.clone();
            let app_handle = app.cloned();

            async move {
                let rt = entry.refresh_token.trim().to_string();
                let hint_email = entry.email.clone();

                let import_op = async {
                    if rt.is_empty() {
                        return Err("Refresh token is empty".to_string());
                    }

                    // 科学防风控微抖动：注入 30ms ~ 90ms 动态散列延迟，打散请求并发时序，杜绝同一毫秒齐射 Google
                    let jitter = (processed_counter.load(Ordering::Relaxed) % 7) as u64 * 10 + 30;
                    tokio::time::sleep(std::time::Duration::from_millis(jitter)).await;

                    let temp_account_id = uuid::Uuid::new_v4().to_string();

                    // 1. 换取 Token（两阶段轻量验证，附带 429 自动退避重试）
                    let mut token_res = None;
                    for attempt in 0..2 {
                        match crate::modules::oauth::refresh_access_token(
                            &rt,
                            Some(&temp_account_id),
                        )
                        .await
                        {
                            Ok(t) => {
                                token_res = Some(t);
                                break;
                            }
                            Err(e)
                                if (e.contains("429") || e.contains("RESOURCE_EXHAUSTED"))
                                    && attempt == 0 =>
                            {
                                crate::modules::logger::log_warn(&format!(
                                    "⚠️ [Batch Import] Hit rate-limit for {}, backing off 1.5s...",
                                    hint_email.as_deref().unwrap_or("account")
                                ));
                                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    let token_res = token_res
                        .ok_or_else(|| "Failed to refresh token after retry".to_string())?;

                    // 2. 获取真实用户信息
                    let user_info = crate::modules::oauth::get_user_info(
                        &token_res.access_token,
                        Some(&temp_account_id),
                    )
                    .await?;
                    let real_email = user_info.email.trim().to_string();
                    let norm_email = real_email.to_lowercase();

                    // 3. 严格幂等判定：检查是否已有该账号（按邮箱或 RT 匹配已有 ID）
                    let existing_id = existing_email_map
                        .get(&norm_email)
                        .cloned()
                        .or_else(|| existing_rt_map.get(&rt).cloned());

                    let account_id =
                        existing_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                    // 3.5 先解析 project（控制面，被封禁账号也返回 200），
                    // 供统一刷新入口 fetch_quota_with_retry 使用。
                    let project_id =
                        crate::proxy::project_resolver::fetch_project_id(&token_res.access_token)
                            .await
                            .ok();

                    let is_gcp_tos = if crate::modules::oauth::is_gcp_enterprise_client(
                        token_res.oauth_client_key.as_deref(),
                    ) {
                        true
                    } else if let Ok(existing_acc) = load_account(&account_id) {
                        existing_acc.token.is_gcp_tos
                    } else {
                        false
                    };

                    // 4. 构造 TokenData 与 Account（若为既有账号，保留原有偏好与配额数据原地更新，杜绝重复凭据文件！）
                    let mut account = if let Ok(mut existing_acc) = load_account(&account_id) {
                        existing_acc.token = crate::models::TokenData::new(
                            token_res.access_token,
                            rt,
                            token_res.expires_in,
                            Some(real_email.clone()),
                            project_id.or(existing_acc.token.project_id.clone()),
                            None,
                            is_gcp_tos,
                            token_res.id_token,
                        )
                        .with_oauth_client_key(token_res.oauth_client_key);
                        existing_acc.name = user_info.get_display_name();
                        existing_acc.email = real_email.clone();
                        existing_acc.update_last_used();
                        existing_acc
                    } else {
                        let token_data = crate::models::TokenData::new(
                            token_res.access_token,
                            rt,
                            token_res.expires_in,
                            Some(real_email.clone()),
                            project_id,
                            None,
                            is_gcp_tos,
                            token_res.id_token,
                        )
                        .with_oauth_client_key(token_res.oauth_client_key);
                        let mut new_acc = crate::models::Account::new(
                            account_id.clone(),
                            real_email.clone(),
                            token_data,
                        );
                        new_acc.name = user_info.get_display_name();
                        new_acc
                    };

                    // 4.1 统一刷新入口：批量导入新账号时强制执行一次数据面封号探针；
                    // 封号直接落盘并跳过配额，未封号才拉取配额。
                    //
                    // 先落盘，确保 `mark_account_banned` / `clear_account_ban`
                    // 在导入阶段也能通过 load_account 正常读写。
                    save_account(&account)?;
                    match crate::modules::account::fetch_quota_with_retry_forced(&mut account).await
                    {
                        Ok(quota) => {
                            account.quota = Some(quota);
                        }
                        Err(e) => {
                            crate::modules::logger::log_warn(&format!(
                                "⚠️ [Batch Import] Unified quota refresh failed for {}: {}",
                                account.email, e
                            ));
                        }
                    }

                    // 5. 单账号独立写入 accounts/{uuid}.json（零全局锁竞争，同时自动清理同邮箱孤儿文件）
                    save_account(&account)?;

                    let summary = crate::models::AccountSummary {
                        id: account.id.clone(),
                        email: account.email.clone(),
                        name: account.name.clone(),
                        disabled: account.disabled,
                        proxy_disabled: account.proxy_disabled,
                        protected_models: account.protected_models.clone(),
                        created_at: account.created_at,
                        last_used: account.last_used,
                    };

                    Ok((account, summary))
                };

                let res = import_op.await;
                let current_processed = processed_counter.fetch_add(1, Ordering::SeqCst) + 1;
                let current_email = match &res {
                    Ok((acc, _)) => {
                        success_counter.fetch_add(1, Ordering::SeqCst);
                        Some(acc.email.clone())
                    }
                    Err(err) => {
                        failed_counter.fetch_add(1, Ordering::SeqCst);
                        let msg =
                            format!("{}: {}", hint_email.as_deref().unwrap_or("unknown"), err);
                        crate::modules::logger::log_warn(&format!(
                            "⚠️ [Batch Import] Failed: {}",
                            msg
                        ));
                        errors_list.lock().await.push(msg);
                        hint_email
                    }
                };

                // 流式下发进度事件至前端（若在桌面环境下）
                if let Some(ref handle) = app_handle {
                    let _ = handle.emit(
                        "accounts://import-progress",
                        crate::models::BatchImportProgress {
                            current: current_processed,
                            total,
                            success: success_counter.load(Ordering::Relaxed),
                            failed: failed_counter.load(Ordering::Relaxed),
                            current_email,
                        },
                    );
                }

                res
            }
        })
        .buffer_unordered(CONCURRENCY);

    let mut successful_summaries = Vec::new();
    let mut stream = Box::pin(stream);

    while let Some(item_res) = stream.next().await {
        if let Ok((_acc, summary)) = item_res {
            successful_summaries.push(summary);
        }
    }

    let success_count = successful_summaries.len();
    let failed_count = failed_counter.load(Ordering::SeqCst);
    let all_errors = errors_list.lock().await.clone();

    // 3. 单次事务化落盘：统一将成功导入的账号摘要合并写入 accounts.json
    if !successful_summaries.is_empty() {
        let _lock = ACCOUNT_INDEX_LOCK
            .lock()
            .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
        let mut index = load_account_index().unwrap_or_default();
        let was_empty = index.accounts.is_empty();

        for s in successful_summaries {
            let s_email_norm = s.email.trim().to_lowercase();
            if let Some(existing) = index
                .accounts
                .iter_mut()
                .find(|a| a.id == s.id || a.email.trim().to_lowercase() == s_email_norm)
            {
                existing.name = s.name;
                existing.email = s.email;
                existing.last_used = s.last_used;
            } else {
                index.accounts.push(s);
            }
        }

        // 防御性去重：确保每个 email 在索引列表中绝对唯一，彻底消除重复条目
        let mut seen = std::collections::HashSet::new();
        index
            .accounts
            .retain(|a| seen.insert(a.email.trim().to_lowercase()));

        if was_empty && index.current_account_id.is_none() {
            index.current_account_id = index.accounts.first().map(|a| a.id.clone());
        }

        save_account_index(&index)?;
        crate::modules::logger::log_info(
            "💾 [Batch Import] Successfully committed new accounts to index file",
        );
    }

    crate::modules::logger::log_info(&format!(
        "🎉 [Batch Import] Finished: {} succeeded, {} failed out of {} total",
        success_count, failed_count, total
    ));

    Ok(crate::models::BatchImportResult {
        total,
        success: success_count,
        failed: failed_count,
        errors: all_errors,
    })
}
