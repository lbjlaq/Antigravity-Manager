use serde::Serialize;
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;
use std::collections::HashSet;

use crate::models::{
    Account, AccountIndex, AccountSummary, DeviceProfile, DeviceProfileVersion, QuotaData,
    TokenData,
};
use crate::modules;
use once_cell::sync::Lazy;
use std::sync::Mutex;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    // Global mutex to prevent concurrent test execution
    static TEST_MUTEX: Lazy<StdMutex<()>> = Lazy::new(|| StdMutex::new(()));

    struct TestDataDir {
        path: PathBuf,
    }

    impl TestDataDir {
        fn new() -> Self {
            let temp_path = std::env::temp_dir().join(format!(
                "antigravity_test_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis()
            ));
            fs::create_dir_all(&temp_path).expect("Failed to create temp dir");
            
            Self {
                path: temp_path,
            }
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
            ),
        );
        
        let content = serde_json::to_string_pretty(&account).expect("Failed to serialize account");
        let account_path = accounts_dir.join(format!("{}.json", account_id));
        fs::write(&account_path, content).expect("Failed to write account file");
    }

    #[test]
    fn test_load_account_index_with_bom_prefix() {
        let _guard = TEST_MUTEX.lock().unwrap();
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
        assert!(result.is_ok(), "BOM should be stripped and JSON should parse: {:?}", result);
        let index = result.unwrap();
        assert!(index.accounts.is_empty());
        println!("BOM case: successfully loaded index after sanitization");
    }

    #[test]
    fn test_load_account_index_with_nul_prefix() {
        let _guard = TEST_MUTEX.lock().unwrap();
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
        assert!(result.is_ok(), "NUL prefix should be stripped and JSON should parse: {:?}", result);
        let index = result.unwrap();
        assert!(index.accounts.is_empty());
        println!("NUL prefix case: successfully loaded index after sanitization");
    }

    #[test]
    fn test_load_account_index_with_garbage_content() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let dir = TestDataDir::new();

        // Non-JSON garbage content - should trigger recovery
        write_corrupted_index(dir.path(), b"\0\0not json");

        let result = load_account_index_in_dir(dir.path());
        
        // New behavior: garbage content triggers recovery, returns empty index
        assert!(result.is_ok(), "Garbage content should trigger recovery and return Ok: {:?}", result);
        let index = result.unwrap();
        assert!(index.accounts.is_empty(), "Recovered index should be empty when no account files exist");
        println!("Garbage content case: successfully recovered to empty index");
    }

    #[test]
    fn test_load_account_index_with_empty_file() {
        let _guard = TEST_MUTEX.lock().unwrap();
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
        let _guard = TEST_MUTEX.lock().unwrap();
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
        let _guard = TEST_MUTEX.lock().unwrap();
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
        assert_eq!(index.accounts.len(), 2, "Index should have 2 accounts recovered from accounts directory");
        
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
        assert_eq!(account_files.len(), 2, "Account files should still exist on disk");
        
        println!("Missing index with existing accounts: successfully recovered {} accounts", index.accounts.len());
    }

    #[test]
    fn test_save_account_index_roundtrip() {
        let _guard = TEST_MUTEX.lock().unwrap();
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
        };

        // Save the index
        save_account_index_in_dir(dir.path(), &index).expect("Failed to save account index");

        // Load it back
        let loaded = load_account_index_in_dir(dir.path()).expect("Failed to load account index");

        // Assert it matches
        assert_eq!(loaded.accounts.len(), 2, "Should have 2 accounts");
        assert_eq!(loaded.current_account_id, Some("acc-1".to_string()), "current_account_id should match");
        
        // Check first account
        let acc1 = loaded.accounts.iter().find(|a| a.id == "acc-1").expect("acc-1 should exist");
        assert_eq!(acc1.email, "user1@example.com");
        assert_eq!(acc1.name, Some("User One".to_string()));
        assert!(!acc1.disabled);
        assert!(!acc1.proxy_disabled);
        
        // Check second account
        let acc2 = loaded.accounts.iter().find(|a| a.id == "acc-2").expect("acc-2 should exist");
        assert_eq!(acc2.email, "user2@example.com");
        assert_eq!(acc2.name, None);
        assert!(acc2.disabled);
        assert!(acc2.proxy_disabled);

        println!("save_account_index roundtrip: successfully saved and loaded index with {} accounts", loaded.accounts.len());
    }

    #[test]
    fn test_backup_created_on_parse_failure() {
        let _guard = TEST_MUTEX.lock().unwrap();
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
        let recovered = load_account_index_in_dir(dir.path()).expect("Should recover from accounts");
        assert_eq!(recovered.accounts.len(), 1, "Should recover 1 account");
        assert_eq!(recovered.accounts[0].email, "recovered@example.com");
        assert_eq!(recovered.current_account_id, Some("recovered-acc".to_string()));

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
        let backup_content = fs::read(&backup_files[0].path()).expect("Should be able to read backup file");
        assert_eq!(backup_content, garbage_content, "Backup should contain original corrupt content");

        println!("Backup creation on parse failure: successfully created backup");
    }
}

/// Global account write lock to prevent corruption during concurrent operations
static ACCOUNT_INDEX_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

// ... existing constants ...
const DATA_DIR: &str = ".antigravity_tools";
const ACCOUNTS_INDEX: &str = "accounts.json";
const ACCOUNTS_DIR: &str = "accounts";

/// Get data directory path
pub fn get_data_dir() -> Result<PathBuf, String> {
    // [NEW] Support custom data directory via environment variable
    if let Ok(env_path) = std::env::var("ABV_DATA_DIR") {
        if !env_path.trim().is_empty() {
            let data_dir = PathBuf::from(env_path);
            if !data_dir.exists() {
                fs::create_dir_all(&data_dir).map_err(|e| format!("failed_to_create_custom_data_dir: {}", e))?;
            }
            return Ok(data_dir);
        }
    }

    let home = dirs::home_dir().ok_or("failed_to_get_home_dir")?;
    let data_dir = home.join(DATA_DIR);

    // Ensure directory exists
    if !data_dir.exists() {
        fs::create_dir_all(&data_dir).map_err(|e| format!("failed_to_create_data_dir: {}", e))?;
    }

    Ok(data_dir)
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
fn load_account_index_in_dir(data_dir: &PathBuf) -> Result<AccountIndex, String> {
    let index_path = data_dir.join(ACCOUNTS_INDEX);

    if !index_path.exists() {
        crate::modules::logger::log_warn(
            "Account index file not found, attempting recovery from accounts directory",
        );
        let recovered = rebuild_index_from_accounts_in_dir(data_dir)?;
        try_save_recovered_index(data_dir, &index_path, &recovered, None)?;
        return Ok(recovered);
    }

    let raw_content = fs::read(&index_path)
        .map_err(|e| format!("failed_to_read_account_index: {}", e))?;

    // If file is empty, attempt recovery
    if raw_content.is_empty() {
        crate::modules::logger::log_warn(
            "Account index is empty, attempting recovery from accounts directory",
        );
        let recovered = rebuild_index_from_accounts_in_dir(data_dir)?;
        try_save_recovered_index(data_dir, &index_path, &recovered, None)?;
        return Ok(recovered);
    }

    // Sanitize content: strip BOM and leading NUL bytes
    let sanitized = sanitize_index_content(&raw_content);

    // If sanitized content is empty/whitespace, attempt recovery
    if sanitized.trim().is_empty() {
        crate::modules::logger::log_warn(
            "Account index is empty after sanitization, attempting recovery from accounts directory",
        );
        let recovered = rebuild_index_from_accounts_in_dir(data_dir)?;
        try_save_recovered_index(data_dir, &index_path, &recovered, None)?;
        return Ok(recovered);
    }

    // Try to parse sanitized content
    match serde_json::from_str::<AccountIndex>(&sanitized) {
        Ok(index) => {
            crate::modules::logger::log_info(&format!(
                "Successfully loaded index with {} accounts",
                index.accounts.len()
            ));
            Ok(index)
        }
        Err(parse_err) => {
            crate::modules::logger::log_error(&format!(
                "Failed to parse account index: {}. Attempting recovery from accounts directory",
                parse_err
            ));
            let recovered = rebuild_index_from_accounts_in_dir(data_dir)?;
            try_save_recovered_index(data_dir, &index_path, &recovered, Some(&raw_content))?;
            Ok(recovered)
        }
    }
}

/// Save account index to a specific directory (internal helper)
fn save_account_index_in_dir(data_dir: &PathBuf, index: &AccountIndex) -> Result<(), String> {
    let index_path = data_dir.join(ACCOUNTS_INDEX);
    // Use unique temp file name per write to avoid collision
    let temp_filename = format!("{}.tmp.{}", ACCOUNTS_INDEX, Uuid::new_v4());
    let temp_path = data_dir.join(&temp_filename);

    let content = serde_json::to_string_pretty(index)
        .map_err(|e| format!("failed_to_serialize_account_index: {}", e))?;

    // Write to temporary file
    if let Err(e) = fs::write(&temp_path, content) {
        // Clean up temp file on failure
        let _ = fs::remove_file(&temp_path);
        return Err(format!("failed_to_write_temp_index_file: {}", e));
    }

    // Atomic rename with platform-specific handling
    if let Err(e) = atomic_replace_file(&temp_path, &index_path) {
        // Clean up temp file on failure
        let _ = fs::remove_file(&temp_path);
        return Err(format!("failed_to_replace_index_file: {}", e));
    }

    Ok(())
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
    })
}

/// Load account from a specific path (internal helper)
fn load_account_at_path(account_path: &PathBuf) -> Result<Account, String> {
    let content = fs::read_to_string(account_path)
        .map_err(|e| format!("failed_to_read_account_data: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("failed_to_parse_account_data: {}", e))
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
        if let Err(e) = fs::write(&backup_path, content) {
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
                "Could not acquire lock to save recovered index. Will retry on next load."
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

/// Platform-specific atomic file replacement
#[cfg(target_os = "windows")]
fn atomic_replace_file(src: &PathBuf, dst: &PathBuf) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    type Bool = i32;
    type Dword = u32;

    #[link(name = "Kernel32")]
    extern "system" {
        fn MoveFileExW(lp_existing_file_name: *const u16, lp_new_file_name: *const u16, dw_flags: Dword) -> Bool;
    }

    let src_wide: Vec<u16> = src
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let dst_wide: Vec<u16> = dst
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // MOVEFILE_REPLACE_EXISTING = 0x1
    // MOVEFILE_WRITE_THROUGH = 0x8
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;

    let result = unsafe { MoveFileExW(src_wide.as_ptr(), dst_wide.as_ptr(), flags) };
    if result == 0 {
        let err = std::io::Error::last_os_error();
        // Clean up source file on failure
        let _ = fs::remove_file(src);
        return Err(format!("MoveFileExW failed: {}", err));
    }

    Ok(())
}

/// Non-Windows: use standard rename
#[cfg(not(target_os = "windows"))]
fn atomic_replace_file(src: &PathBuf, dst: &PathBuf) -> Result<(), String> {
    fs::rename(src, dst).map_err(|e| format!("rename failed: {}", e))
}

/// Load account data
pub fn load_account(account_id: &str) -> Result<Account, String> {
    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account_id));
    load_account_at_path(&account_path)
}

/// Save account data
pub fn save_account(account: &Account) -> Result<(), String> {
    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account.id));

    let temp_filename = format!("{}.tmp.{}", account.id, Uuid::new_v4());
    let temp_path = accounts_dir.join(&temp_filename);

    let content = serde_json::to_string_pretty(account)
        .map_err(|e| format!("failed_to_serialize_account_data: {}", e))?;

    if let Err(e) = std::fs::write(&temp_path, content) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("failed_to_write_temp_account_file: {}", e));
    }

    if let Err(e) = atomic_replace_file(&temp_path, &account_path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("failed_to_replace_account_file: {}", e));
    }

    Ok(())
}

/// List all accounts
pub fn list_accounts() -> Result<Vec<Account>, String> {
    crate::modules::logger::log_info("Listing accounts...");
    let index = load_account_index()?;
    let mut accounts = Vec::new();

    for summary in &index.accounts {
        match load_account(&summary.id) {
            Ok(account) => accounts.push(account),
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

    Ok(accounts)
}

/// Add account
pub fn add_account(
    email: String,
    name: Option<String>,
    token: TokenData,
) -> Result<Account, String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
    let mut index = load_account_index()?;

    // Check if account already exists
    if index.accounts.iter().any(|s| s.email == email) {
        return Err(format!("Account already exists: {}", email));
    }

    // Create new account
    let account_id = Uuid::new_v4().to_string();
    let mut account = Account::new(account_id.clone(), email.clone(), token);
    account.name = name.clone();

    // Save account data
    save_account(&account)?;

    // Update index
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

    // If first account, set as current
    if index.current_account_id.is_none() {
        index.current_account_id = Some(account_id);
    }

    save_account_index(&index)?;

    Ok(account)
}

/// Add or update account
pub fn upsert_account(
    email: String,
    name: Option<String>,
    token: TokenData,
) -> Result<Account, String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
    let mut index = load_account_index()?;

    // Find account ID if exists
    let existing_account_id = index
        .accounts
        .iter()
        .find(|s| s.email == email)
        .map(|s| s.id.clone());

    if let Some(account_id) = existing_account_id {
        // Update existing account
        match load_account(&account_id) {
            Ok(mut account) => {
                let old_access_token = account.token.access_token.clone();
                let old_refresh_token = account.token.refresh_token.clone();
                account.token = token;
                account.name = name.clone();
                // If an account was previously disabled (e.g. invalid_grant), any explicit token upsert
                // should re-enable it (user manually updated credentials in the UI).
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

                // Sync name in index
                if let Some(idx_summary) = index.accounts.iter_mut().find(|s| s.id == account_id) {
                    idx_summary.name = name;
                    save_account_index(&index)?;
                }

                return Ok(account);
            }
            Err(e) => {
                crate::modules::logger::log_warn(&format!(
                    "Account {} file missing ({}), recreating...",
                    account_id, e
                ));
                // Index exists but file is missing, recreating
                let mut account = Account::new(account_id.clone(), email.clone(), token);
                account.name = name.clone();
                save_account(&account)?;

                // Sync name in index
                if let Some(idx_summary) = index.accounts.iter_mut().find(|s| s.id == account_id) {
                    idx_summary.name = name;
                    save_account_index(&index)?;
                }

                return Ok(account);
            }
        }
    }

    // Add if not exists
    // Note: add_account will attempt to acquire lock, which would deadlock here.
    // Use an internal version or release lock.

    // Release lock, let add_account handle it
    drop(_lock);
    add_account(email, name, token)
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

    if index.accounts.len() == original_len {
        return Err(format!("Account ID not found: {}", account_id));
    }

    // Clear current account if it's being deleted
    if index.current_account_id.as_deref() == Some(account_id) {
        index.current_account_id = index.accounts.first().map(|s| s.id.clone());
    }

    save_account_index(&index)?;

    // Delete account file
    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account_id));

    if account_path.exists() {
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
        "Switching to account: {} (ID: {})",
        account.email, account.id
    ));

    // 2. Ensure Token is valid (auto-refresh)
    let fresh_token = oauth::ensure_fresh_token(&account.token, Some(&account.id))
        .await
        .map_err(|e| format!("Token refresh failed: {}", e))?;

    // If Token updated, save back to account file
    if fresh_token.access_token != account.token.access_token {
        account.token = fresh_token.clone();
        save_account(&account)?;
    }

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
    integration.on_account_switch(&account).await?;

    // 4. Update tool internal state
    {
        let _lock = ACCOUNT_INDEX_LOCK
            .lock()
            .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
        let mut index = load_account_index()?;
        index.current_account_id = Some(account_id.to_string());
        save_account_index(&index)?;
    }

    account.update_last_used();
    save_account(&account)?;

    crate::modules::logger::log_info(&format!(
        "Account switch core logic completed: {}",
        account.email
    ));

    Ok(())
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
    let current = crate::modules::device::get_storage_path()
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
        "capture" => device::read_profile(&device::get_storage_path()?)?,
        "generate" => device::generate_profile(),
        _ => return Err("mode must be 'capture' or 'generate'".to_string()),
    };

    let mut account = load_account(account_id)?;
    let _ = device::save_global_original(&profile);
    apply_profile_to_account(
        &mut account, profile.clone(), Some(mode.to_string()), true)?;

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
    let storage_path = device::get_storage_path()?;
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
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
    let mut index = load_account_index()?;
    index.current_account_id = Some(account_id.to_string());
    save_account_index(&index)
}

/// Update account quota
pub fn update_account_quota(account_id: &str, quota: QuotaData) -> Result<(), String> {
    let mut account = load_account(account_id)?;
    account.update_quota(quota);

    // --- Quota protection logic start ---
    if let Ok(config) = crate::modules::config::load_app_config() {
        if config.quota_protection.enabled {
            if let Some(ref q) = account.quota {
                let threshold = config.quota_protection.threshold_percentage as i32;

                let mut group_min_percentage: HashMap<String, i32> = HashMap::new();

                for model in &q.models {
                    if let Some(std_id) =
                        crate::proxy::common::model_mapping::normalize_to_standard_id(&model.name)
                    {
                        let entry = group_min_percentage.entry(std_id).or_insert(100);
                        if model.percentage < *entry {
                            *entry = model.percentage;
                        }
                    }
                }

                for std_id in &config.quota_protection.monitored_models {
                    let min_pct = group_min_percentage.get(std_id).cloned().unwrap_or(100);

                    if min_pct <= threshold {
                        if !account.protected_models.contains(std_id) {
                            crate::modules::logger::log_info(&format!(
                                "[Quota] Triggering model protection: {} (Group: {} Min: {}% <= Thres: {}%)",
                                account.email, std_id, min_pct, threshold
                            ));
                            account.protected_models.insert(std_id.clone());
                        }
                    } else {
                        if account.protected_models.contains(std_id) {
                            crate::modules::logger::log_info(&format!(
                                "[Quota] Model protection recovered: {} (Group: {} Min: {}% > Thres: {}%)",
                                account.email, std_id, min_pct, threshold
                            ));
                            account.protected_models.remove(std_id);
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
        }
    }
    // --- Quota protection logic end ---

    // Save account first
    save_account(&account)?;

    // [FIX] 同时更新索引文件中的摘要信息，确保列表页图标即时刷新
    {
        let _lock = ACCOUNT_INDEX_LOCK
            .lock()
            .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
        if let Ok(mut index) = load_account_index() {
            if let Some(summary) = index.accounts.iter_mut().find(|a| a.id == account_id) {
                summary.protected_models = account.protected_models.clone();
                let _ = save_account_index(&index);
            }
        }
    }

    // [FIX] Trigger TokenManager account reload signal
    // This ensures in-memory protected_models are updated
    crate::proxy::server::trigger_account_reload(account_id);

    Ok(())
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
    load_account_index().ok()?.accounts.into_iter()
        .find(|a| a.email == email)
        .map(|a| a.id)
}

pub fn mark_account_forbidden(account_id: &str, reason: &str) -> Result<(), String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;

    let mut account = load_account(account_id)?;

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
        });
    }

    // 2. Disable proxy for this account
    account.proxy_disabled = true;
    account.proxy_disabled_reason = Some(format!("Forbidden (403): {}", reason));
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

/// Export accounts by IDs (for backup/migration)
pub fn export_accounts_by_ids(account_ids: &[String]) -> Result<crate::models::AccountExportResponse, String> {
    use crate::models::{AccountExportItem, AccountExportResponse};
    
    let accounts = list_accounts()?;
    
    let export_items: Vec<AccountExportItem> = accounts
        .into_iter()
        .filter(|acc| account_ids.contains(&acc.id))
        .map(|acc| AccountExportItem {
            email: acc.email,
            refresh_token: acc.token.refresh_token,
        })
        .collect();

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
    use reqwest::StatusCode;

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

    // 2. Attempt query
    let result: crate::error::AppResult<(QuotaData, Option<String>)> =
        modules::fetch_quota(&account.token.access_token, &account.email, Some(&account.id)).await;

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
                let token_res = match oauth::refresh_access_token(&account.token.refresh_token, Some(&account.id))
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

                // Retry query
                let retry_result: crate::error::AppResult<(QuotaData, Option<String>)> =
                    modules::fetch_quota(&new_token.access_token, &account.email, Some(&account.id)).await;

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
                return retry_result.map(|(q, _)| q);
            }
        }
    }

    // fetch_quota already handles 403, just return mapping result
    result.map(|(q, _)| q)
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
    use futures::future::join_all;
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

    let tasks: Vec<_> = accounts
        .into_iter()
        .filter(|account| {
            if account.disabled || account.proxy_disabled {
                crate::modules::logger::log_info(&format!(
                    "  - Skipping {} ({})",
                    account.email,
                    if account.disabled { "Disabled" } else { "Proxy Disabled" }
                ));
                return false;
            }
            // [FIX] Check proxy_disabled status
            if account.proxy_disabled {
                crate::modules::logger::log_info(&format!(
                    "  - Skipping {} (Proxy Disabled)",
                    account.email
                ));
                return false;
            }
            if let Some(ref q) = account.quota {
                if q.is_forbidden {
                    crate::modules::logger::log_info(&format!(
                        "  - Skipping {} (Forbidden)",
                        account.email
                    ));
                    return false;
                }
            }
            true
        })
        .map(|mut account| {
            let email = account.email.clone();
            let account_id = account.id.clone();
            let permit = semaphore.clone();
            async move {
                let _guard = permit.acquire().await.unwrap();
                crate::modules::logger::log_info(&format!("  - Processing {}", email));
                match fetch_quota_with_retry(&mut account).await {
                    Ok(quota) => {
                        if let Err(e) = update_account_quota(&account_id, quota) {
                            let msg = format!("Account {}: Save quota failed - {}", email, e);
                            crate::modules::logger::log_error(&msg);
                            Err(msg)
                        } else {
                            crate::modules::logger::log_info(&format!("    Success {}", email));
                            Ok(())
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
        .collect();

    let total = tasks.len();
    let results = join_all(tasks).await;

    let mut success = 0;
    let mut failed = 0;
    let mut details = Vec::new();

    for result in results {
        match result {
            Ok(()) => success += 1,
            Err(msg) => {
                failed += 1;
                details.push(msg);
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

    // After quota refresh, immediately check and trigger warmup for recovered models
    // [Disabled] Automatic warmup is temporarily disabled
    // tokio::spawn(async {
    //     check_and_trigger_warmup_for_recovered_models().await;
    // });

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
