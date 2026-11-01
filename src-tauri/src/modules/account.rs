use serde::Serialize;
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

use crate::models::{
    Account, AccountIndex, AccountSummary, DeviceProfile, DeviceProfileVersion, QuotaData,
    TokenData,
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
    crate::models::account::validate_priority(priority)?;
    let _account_write = lock_account_file_updates()?;
    let mut account = load_account(account_id)?;
    account.priority = priority;
    save_account(&account)
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

    // Remove from index
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
    client_key
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
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(reason) {
        let metadata_url = parsed
            .get("error")
            .and_then(|e| e.get("details"))
            .and_then(|d| d.as_array())
            .and_then(|arr| arr.iter().find_map(|item| item.get("metadata")))
            .and_then(|m| m.get("validation_url").or_else(|| m.get("appeal_url")))
            .and_then(|u| u.as_str())
            .map(|s| s.replace("\\u0026", "&"));

        if metadata_url.is_some() {
            return metadata_url;
        }

        let link_url = parsed
            .get("error")
            .and_then(|e| e.get("details"))
            .and_then(|d| d.as_array())
            .and_then(|arr| arr.iter().find_map(|item| item.get("links")))
            .and_then(|links| links.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("url"))
            .and_then(|u| u.as_str())
            .map(|s| s.replace("\\u0026", "&"));

        if link_url.is_some() {
            return link_url;
        }
    }

    let url_regex = regex::Regex::new(r#"https://[^\s"'\\]+"#).ok()?;
    url_regex.find(reason).map(|m| {
        let raw_url = m.as_str().to_string();
        raw_url.replace("\\u0026", "&")
    })
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
        q.forbidden_reason = Some(reason.to_string());
    }

    if let Err(e) = save_account(account) {
        crate::modules::logger::log_warn(&format!(
            "Failed to persist validation_blocked state for {}: {}",
            account.email, e
        ));
    }
}

fn clear_validation_blocked(account: &mut Account) {
    if !account.validation_blocked
        && account.validation_blocked_until.is_none()
        && account.validation_blocked_reason.is_none()
        && account.validation_url.is_none()
        && !account.proxy_disabled
    {
        return;
    }

    account.validation_blocked = false;
    account.validation_blocked_until = None;
    account.validation_blocked_reason = None;
    account.validation_url = None;
    if let Some(ref mut q) = account.quota {
        q.is_forbidden = false;
        q.forbidden_reason = None;
    }

    // Auto-recover proxy_disabled if it was caused by 403 or validation
    if account.proxy_disabled
        && account.proxy_disabled_reason.as_ref().map_or(false, |r| {
            r.starts_with("Forbidden (403)")
                || r.contains("403 Forbidden")
                || r.contains("VALIDATION_REQUIRED")
        })
    {
        account.proxy_disabled = false;
        account.proxy_disabled_reason = None;
        account.proxy_disabled_at = None;
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
    index.current_target_ide = target_ide.map(|s| s.to_string());
    match target_ide {
        Some("classic") => index.current_classic_account_id = Some(account_id.to_string()),
        Some("ide") => index.current_ide_account_id = Some(account_id.to_string()),
        Some("agy") => index.current_agy_account_id = Some(account_id.to_string()),
        _ => {
            index.current_classic_account_id = Some(account_id.to_string());
        }
    }
    save_account_index(&index)
}

/// 智能探查并获取经典款、Antigravity IDE、Antigravity CLI 三大客户端当前各自生效的账号状态
pub fn detect_active_clients_status() -> Result<crate::models::ActiveClientsStatus, String> {
    let index = load_account_index()?;
    let mut status = crate::models::ActiveClientsStatus {
        classic_account_id: index.current_classic_account_id.clone(),
        classic_email: None,
        ide_account_id: index.current_ide_account_id.clone(),
        ide_email: None,
        agy_account_id: index.current_agy_account_id.clone(),
        agy_email: None,
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

    // Fallback if index has current_account_id and current_target_ide
    if status.classic_account_id.is_none()
        && status.ide_account_id.is_none()
        && status.agy_account_id.is_none()
    {
        if let Some(ref curr_id) = index.current_account_id {
            match index.current_target_ide.as_deref() {
                Some("ide") => status.ide_account_id = Some(curr_id.clone()),
                Some("agy") => status.agy_account_id = Some(curr_id.clone()),
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

    // If quota retrieval succeeded and quota is not forbidden, auto-recover 403 / validation locks
    if let Some(ref q) = account.quota {
        if !q.is_forbidden {
            clear_validation_blocked(&mut account);
            if account.proxy_disabled
                && account.proxy_disabled_reason.as_ref().map_or(false, |r| {
                    r == "quota_protection"
                        || r.starts_with("Forbidden (403)")
                        || r.contains("403 Forbidden")
                        || r.contains("VALIDATION_REQUIRED")
                })
            {
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

/// Toggle proxy disabled status for an account
pub fn toggle_proxy_status(
    account_id: &str,
    enable: bool,
    reason: Option<&str>,
) -> Result<(), String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;

    let mut account = load_account(account_id)?;

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

pub fn mark_account_forbidden(account_id: &str, reason: &str) -> Result<(), String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;

    let mut account = load_account(account_id)?;

    // Extract action URL (appeal_url or validation_url)
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
            last_updated: chrono::Utc::now().timestamp(),
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
    account.proxy_disabled_at = Some(chrono::Utc::now().timestamp());

    save_account(&account)?;

    // 3. Update index summary
    let mut index = load_account_index()?;
    if let Some(summary) = index.accounts.iter_mut().find(|a| a.id == account_id) {
        summary.proxy_disabled = true;
        save_account_index(&index)?;
    }

    // 4. Notify frontend to refresh account list
    crate::modules::log_bridge::emit_accounts_refreshed();

    Ok(())
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

/// Quota query with retry (moved from commands to modules for reuse)
pub async fn fetch_quota_with_retry(account: &mut Account) -> crate::error::AppResult<QuotaData> {
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

    // 2. Attempt query (pass cached project_id if available to avoid unnecessary loadCodeAssist)
    let result: crate::error::AppResult<(QuotaData, Option<String>)> =
        modules::fetch_quota_with_cache(
            &account.token.access_token,
            &account.email,
            account.token.project_id.as_deref(),
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

                let new_token = TokenData::new(
                    token_res.access_token.clone(),
                    account.token.refresh_token.clone(),
                    token_res.expires_in,
                    account.token.email.clone(),
                    account.token.project_id.clone(), // Keep original project_id
                    None,                             // Add None as session_id
                    account.token.is_gcp_tos,
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
                            return Ok(q);
                        }
                    }
                }

                match retry_result {
                    Ok((q, _)) => {
                        if !q.is_forbidden {
                            clear_validation_blocked(account);
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
            if !q.is_forbidden {
                clear_validation_blocked(account);
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
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    const MAX_CONCURRENT: usize = 5;
    let start = std::time::Instant::now();

    crate::modules::logger::log_info(&format!(
        "Starting batch refresh of all account quotas (Concurrent mode, max: {})",
        MAX_CONCURRENT
    ));
    let accounts = list_accounts()?;

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));

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
                    let until = account.validation_blocked_until.unwrap_or(0);
                    if account.validation_blocked && now < until {
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

    // [FIX #3726] 使用流式异步消费替代 join_all，内存中同时挂起并运行的任务严格受控为 MAX_CONCURRENT，杜绝内存堆积
    let mut stream = stream::iter(filtered_accounts)
        .map(|mut account| {
            let email = account.email.clone();
            let account_id = account.id.clone();
            let permit = semaphore.clone();
            async move {
                let _guard = permit.acquire().await.unwrap();
                crate::modules::logger::log_info(&format!("  - Processing {}", email));
                match fetch_quota_with_retry(&mut account).await {
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
                }
            }
        })
        .buffer_unordered(MAX_CONCURRENT);

    let mut success = 0;
    let mut failed = 0;
    let mut details = Vec::new();
    let mut updated_accounts = Vec::new();

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
        if account.disabled || account.proxy_disabled {
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

                    // 4. [NEW #3726] 探测 Project ID 与拉取完备配额（满血持久化，确保导入完成后冷启动秒加载！）
                    let project_id =
                        crate::proxy::project_resolver::fetch_project_id(&token_res.access_token)
                            .await
                            .ok();

                    let quota_res = crate::modules::quota::fetch_quota(
                        &token_res.access_token,
                        &real_email,
                        Some(&account_id),
                    )
                    .await;

                    // 5. 构造 TokenData 与 Account（若为既有账号，保留原有偏好与配额数据原地更新，杜绝重复凭据文件！）
                    let mut account = if let Ok(mut existing_acc) = load_account(&account_id) {
                        existing_acc.token = crate::models::TokenData::new(
                            token_res.access_token,
                            rt,
                            token_res.expires_in,
                            Some(real_email.clone()),
                            project_id.or(existing_acc.token.project_id.clone()),
                            None,
                            false,
                            token_res.id_token,
                        )
                        .with_oauth_client_key(token_res.oauth_client_key);
                        existing_acc.name = user_info.get_display_name();
                        existing_acc.email = real_email.clone();
                        existing_acc.update_last_used();
                        if let Ok((quota_data, new_pid)) = quota_res {
                            existing_acc.quota = Some(quota_data);
                            if let Some(pid) = new_pid {
                                existing_acc.token.project_id = Some(pid);
                            }
                        }
                        existing_acc
                    } else {
                        let token_data = crate::models::TokenData::new(
                            token_res.access_token,
                            rt,
                            token_res.expires_in,
                            Some(real_email.clone()),
                            project_id,
                            None,
                            false,
                            token_res.id_token,
                        )
                        .with_oauth_client_key(token_res.oauth_client_key);
                        let mut new_acc = crate::models::Account::new(
                            account_id.clone(),
                            real_email.clone(),
                            token_data,
                        );
                        new_acc.name = user_info.get_display_name();
                        if let Ok((quota_data, new_pid)) = quota_res {
                            new_acc.quota = Some(quota_data);
                            if let Some(pid) = new_pid {
                                new_acc.token.project_id = Some(pid);
                            }
                        }
                        new_acc
                    };

                    // 6. 单账号独立写入 accounts/{uuid}.json（零全局锁竞争，同时自动清理同邮箱孤儿文件）
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
