use std::fs;
use std::path::PathBuf;
use serde_json;
use uuid::Uuid;
use serde::Serialize;

use crate::models::{Account, AccountIndex, AccountSummary, TokenData, QuotaData, DeviceProfile, DeviceProfileVersion,};
use crate::modules;
use once_cell::sync::Lazy;
use std::sync::Mutex;

/// Global account write lock to prevent corruption during concurrent operations
static ACCOUNT_INDEX_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

// ... existing constants ...
const DATA_DIR: &str = ".antigravity_tools";
const ACCOUNTS_INDEX: &str = "accounts.json";
const ACCOUNTS_DIR: &str = "accounts";

// ... existing functions get_data_dir, get_accounts_dir, load_account_index, save_account_index ...
/// Get data directory path
pub fn get_data_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("failed_to_get_home_dir")?;
    let data_dir = home.join(DATA_DIR);
    
    // Ensure directory exists
    if !data_dir.exists() {
        fs::create_dir_all(&data_dir)
            .map_err(|e| format!("failed_to_create_data_dir: {}", e))?;
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

/// Load account index
pub fn load_account_index() -> Result<AccountIndex, String> {
    let data_dir = get_data_dir()?;
    let index_path = data_dir.join(ACCOUNTS_INDEX);
    
    if !index_path.exists() {
        crate::modules::logger::log_warn("Account index file not found");
        return Ok(AccountIndex::new());
    }
    
    let content = fs::read_to_string(&index_path)
        .map_err(|e| format!("failed_to_read_account_index: {}", e))?;
    
    // If file content is empty, treat as new index
    if content.trim().is_empty() {
        crate::modules::logger::log_warn("Account index is empty, initializing new index");
        return Ok(AccountIndex::new());
    }
    
    let index: AccountIndex = serde_json::from_str(&content)
        .map_err(|e| format!("failed_to_parse_account_index: {}", e))?;
        
    crate::modules::logger::log_info(&format!("Successfully loaded index with {} accounts", index.accounts.len()));
    Ok(index)
}

/// Save account index (atomic write)
pub fn save_account_index(index: &AccountIndex) -> Result<(), String> {
    let data_dir = get_data_dir()?;
    let index_path = data_dir.join(ACCOUNTS_INDEX);
    let temp_path = data_dir.join(format!("{}.tmp", ACCOUNTS_INDEX));
    
    let content = serde_json::to_string_pretty(index)
        .map_err(|e| format!("failed_to_serialize_account_index: {}", e))?;
    
    // Write to temporary file
    fs::write(&temp_path, content)
        .map_err(|e| format!("failed_to_write_temp_index_file: {}", e))?;
        
    // Atomic rename
    fs::rename(temp_path, index_path)
        .map_err(|e| format!("failed_to_replace_index_file: {}", e))
}

/// Load account data
pub fn load_account(account_id: &str) -> Result<Account, String> {
    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account_id));
    
    if !account_path.exists() {
        return Err(format!("Account not found: {}", account_id));
    }
    
    let content = fs::read_to_string(&account_path)
        .map_err(|e| format!("failed_to_read_account_data: {}", e))?;
    
    serde_json::from_str(&content)
        .map_err(|e| format!("failed_to_parse_account_data: {}", e))
}

/// Save account data
pub fn save_account(account: &Account) -> Result<(), String> {
    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account.id));
    
    let content = serde_json::to_string_pretty(account)
        .map_err(|e| format!("failed_to_serialize_account_data: {}", e))?;
    
    fs::write(&account_path, content)
        .map_err(|e| format!("failed_to_save_account_data: {}", e))
}

/// List all accounts
pub fn list_accounts() -> Result<Vec<Account>, String> {
    crate::modules::logger::log_info("Listing accounts...");
    let mut index = load_account_index()?;
    let mut accounts = Vec::new();
    let mut invalid_ids = Vec::new();
    
    for summary in &index.accounts {
        match load_account(&summary.id) {
            Ok(account) => accounts.push(account),
            Err(e) => {
                crate::modules::logger::log_error(&format!("Failed to load account {}: {}", summary.id, e));
                // If the error is caused by a missing file, mark as invalid ID
                if e.contains("Account not found") || e.contains("Os { code: 2,") || e.contains("No such file") {
                    invalid_ids.push(summary.id.clone());
                }
            },
        }
    }
    
    // Auto-repair index: remove invalid account IDs
    if !invalid_ids.is_empty() {
        crate::modules::logger::log_warn(&format!("Found {} invalid account indexes, auto-cleaning...", invalid_ids.len()));
        
        index.accounts.retain(|s| !invalid_ids.contains(&s.id));
        
        // If current account is invalid, reset to first available
        if let Some(current_id) = &index.current_account_id {
            if invalid_ids.contains(current_id) {
                index.current_account_id = index.accounts.first().map(|s| s.id.clone());
            }
        }
        
        if let Err(e) = save_account_index(&index) {
            crate::modules::logger::log_error(&format!("Failed to auto-clean index: {}", e));
        } else {
            crate::modules::logger::log_info("Account index auto-cleaned");
        }
    }
    
    Ok(accounts)
}

/// Add account
pub fn add_account(email: String, name: Option<String>, token: TokenData) -> Result<Account, String> {
    let _lock = ACCOUNT_INDEX_LOCK.lock().map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
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
        id: account_id.clone(),
        email: email.clone(),
        name: name.clone(),
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
pub fn upsert_account(email: String, name: Option<String>, token: TokenData) -> Result<Account, String> {
    let _lock = ACCOUNT_INDEX_LOCK.lock().map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
    let mut index = load_account_index()?;
    
    // Find account ID if exists
    let existing_account_id = index.accounts.iter()
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
            },
            Err(e) => {
                crate::modules::logger::log_warn(&format!("Account {} file missing ({}), recreating...", account_id, e));
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
    let _lock = ACCOUNT_INDEX_LOCK.lock().map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
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
    
    Ok(())
}

/// Batch delete accounts (atomic index operation)
pub fn delete_accounts(account_ids: &[String]) -> Result<(), String> {
    let _lock = ACCOUNT_INDEX_LOCK.lock().map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
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
    let _lock = ACCOUNT_INDEX_LOCK.lock().map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
    let mut index = load_account_index()?;
    
    // Create a map of account ID to summary
    let id_to_summary: std::collections::HashMap<_, _> = index.accounts
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
    
    crate::modules::logger::log_info(&format!("Account order updated, {} accounts total", index.accounts.len()));
    
    save_account_index(&index)
}

/// Switch current account
pub async fn switch_account(account_id: &str) -> Result<(), String> {
    use crate::modules::{oauth, process, db, device};
    
    let index = {
        let _lock = ACCOUNT_INDEX_LOCK.lock().map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
        load_account_index()?
    };
    
    // 1. Verify account exists
    if !index.accounts.iter().any(|s| s.id == account_id) {
        return Err(format!("Account not found: {}", account_id));
    }
    
    let mut account = load_account(account_id)?;
    crate::modules::logger::log_info(&format!("Switching to account: {} (ID: {})", account.email, account.id));
    
    // 2. Ensure Token is valid (auto-refresh)
    let fresh_token = oauth::ensure_fresh_token(&account.token).await
        .map_err(|e| format!("Token refresh failed: {}", e))?;
        
    // If Token updated, save back to account file
    if fresh_token.access_token != account.token.access_token {
        account.token = fresh_token.clone();
        save_account(&account)?;
    }
    
    // 3. Get storage path while process is potentially running (to capture --user-data-dir)
    let storage_path = device::get_storage_path()?;

    // 4. Close Antigravity (increase timeout to 20s)
    if process::is_antigravity_running() {
        process::close_antigravity(20)?;
    }

    // 5. Write device profile (generate/bind if missing), only update storage on switch
    let profile_to_apply = {
        if let Some(p) = account.device_profile.clone() {
            p
        } else {
            // [FIX] If no bound profile, generate and bind one for isolation
            // This ensures each account gets a unique fingerprint upon first switch
            crate::modules::logger::log_info(&format!("Account {} has no bound fingerprint, generating new one for isolation...", account.email));
            let new_profile = device::generate_profile();
            // Bind to account and save
            apply_profile_to_account(&mut account, new_profile.clone(), Some("auto_generated".to_string()), true)?;
            new_profile
        }
    };
    crate::modules::logger::log_info(&format!(
        "Writing device profile to storage.json: machineId={}, macMachineId={}, devDeviceId={}, sqmId={}",
        profile_to_apply.machine_id,
        profile_to_apply.mac_machine_id,
        profile_to_apply.dev_device_id,
        profile_to_apply.sqm_id
    ));
    device::write_profile(&storage_path, &profile_to_apply)?;

    // 5. Get database path and backup
    let db_path = db::get_db_path()?;
    if db_path.exists() {
        let backup_path = db_path.with_extension("vscdb.backup");
        fs::copy(&db_path, &backup_path)
            .map_err(|e| format!("failed_to_backup_database: {}", e))?;
    } else {
        crate::modules::logger::log_info("Database does not exist, skipping backup");
    }

    // 6. Inject Token
    crate::modules::logger::log_info("Injecting Token into database...");
    db::inject_token(
        &db_path,
        &account.token.access_token,
        &account.token.refresh_token,
        account.token.expiry_timestamp,
    )?;

    // 7. Update tool internal state
    {
        let _lock = ACCOUNT_INDEX_LOCK.lock().map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
        let mut index = load_account_index()?;
        index.current_account_id = Some(account_id.to_string());
        save_account_index(&index)?;
    }
    
    account.update_last_used();
    save_account(&account)?;

    // 8. Restart Antigravity
    process::start_antigravity()?;
    crate::modules::logger::log_info(&format!("Account switch completed: {}", account.email));

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
    let storage_path = crate::modules::device::get_storage_path()?;
    let current = crate::modules::device::read_profile(&storage_path).ok();
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
    apply_profile_to_account(&mut account, profile.clone(), Some(mode.to_string()), true)?;

    Ok(profile)
}

/// Bind directly with provided profile
pub fn bind_device_profile_with_profile(account_id: &str, profile: DeviceProfile, label: Option<String>) -> Result<DeviceProfile, String> {
    let mut account = load_account(account_id)?;
    let _ = crate::modules::device::save_global_original(&profile);
    apply_profile_to_account(&mut account, profile.clone(), label, true)?;

    Ok(profile)
}

fn apply_profile_to_account(account: &mut Account, profile: DeviceProfile, label: Option<String>, add_history: bool) -> Result<(), String> {
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
        account.device_profile.clone().ok_or("No currently bound profile")?
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
    if account.device_history.iter().any(|v| v.id == version_id && v.is_current) {
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
                return Ok("Reset current account bound profile to original (not applied to storage)".to_string());
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
    let _lock = ACCOUNT_INDEX_LOCK.lock().map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
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


                for model in &q.models {
                    // Normalize model name to standard ID
                    let standard_id = match crate::proxy::common::model_mapping::normalize_to_standard_id(&model.name) {
                        Some(id) => id,
                        None => continue, // Skip if not one of the 3 protected models
                    };
                    
                    // Only monitor models selected by user
                    if !config.quota_protection.monitored_models.contains(&standard_id) {
                        continue;
                    }
                    
                    if model.percentage <= threshold {
                        // Trigger model-level protection
                        if !account.protected_models.contains(&standard_id) {
                            crate::modules::logger::log_info(&format!(
                                "[Quota] Triggering model protection: {} ({} [{}] remaining {}% <= threshold {}%)",
                                account.email, standard_id, model.name, model.percentage, threshold
                            ));
                            account.protected_models.insert(standard_id.clone());

                        }
                    } else {
                        // Auto-recover single model
                        if account.protected_models.contains(&standard_id) {
                            crate::modules::logger::log_info(&format!(
                                "[Quota] Model protection recovered: {} ({} [{}] quota restored to {}%)",
                                account.email, standard_id, model.name, model.percentage
                            ));
                            account.protected_models.remove(&standard_id);

                        }
                    }
                }

                // [Compatibility] Migrate from account-level to model-level protection if previously disabled for quota
                if account.proxy_disabled && 
                   account.proxy_disabled_reason.as_ref().map_or(false, |r| r == "quota_protection") {
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

    save_account(&account)
}

/// Export all accounts' refresh_tokens
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
    use crate::modules::oauth;
    use crate::error::AppError;
    use reqwest::StatusCode;
    
    // 1. Time-based check - ensure Token is valid first
    let token = match oauth::ensure_fresh_token(&account.token).await {
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
            }
            return Err(AppError::OAuth(e));
        }
    };
    
    if token.access_token != account.token.access_token {
        modules::logger::log_info(&format!("Time-based Token refresh: {}", account.email));
        account.token = token.clone();
        
        // Get display name (incidental to Token refresh)
        let name = if account.name.is_none() || account.name.as_ref().map_or(false, |n| n.trim().is_empty()) {
            match oauth::get_user_info(&token.access_token).await {
                Ok(user_info) => user_info.get_display_name(),
                Err(_) => None
            }
        } else {
            account.name.clone()
        };
        
        account.name = name.clone();
        upsert_account(account.email.clone(), name, token.clone()).map_err(AppError::Account)?;
    }

    // 0. Supplement display name (if missing or upper step failed)
    if account.name.is_none() || account.name.as_ref().map_or(false, |n| n.trim().is_empty()) {
        modules::logger::log_info(&format!("Account {} missing display name, attempting to fetch...", account.email));
        // Use updated token
        match oauth::get_user_info(&account.token.access_token).await {
            Ok(user_info) => {
                let display_name = user_info.get_display_name();
                modules::logger::log_info(&format!("Successfully fetched display name: {:?}", display_name));
                account.name = display_name.clone();
                // Save immediately
                if let Err(e) = upsert_account(account.email.clone(), display_name, account.token.clone()) {
                     modules::logger::log_warn(&format!("Failed to save display name: {}", e));
                }
            },
            Err(e) => {
                 modules::logger::log_warn(&format!("Failed to fetch display name: {}", e));
            }
        }
    }

    // 2. Attempt query
    let result: crate::error::AppResult<(QuotaData, Option<String>)> = modules::fetch_quota(&account.token.access_token, &account.email).await;
    
    // Capture potentially updated project_id and save
    if let Ok((ref _q, ref project_id)) = result {
        if project_id.is_some() && *project_id != account.token.project_id {
            modules::logger::log_info(&format!("Detected project_id update ({}), saving...", account.email));
            account.token.project_id = project_id.clone();
            if let Err(e) = upsert_account(account.email.clone(), account.name.clone(), account.token.clone()) {
                modules::logger::log_warn(&format!("Failed to sync project_id: {}", e));
            }
        }
    }

    // 3. Handle 401 error
    if let Err(AppError::Network(ref e)) = result {
        if let Some(status) = e.status() {
            if status == StatusCode::UNAUTHORIZED {
                modules::logger::log_warn(&format!("401 Unauthorized for {}, forcing refresh...", account.email));
                
                // Force refresh
                let token_res = match oauth::refresh_access_token(&account.token.refresh_token).await {
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
                    None, // Add None as session_id
                );
                
                // Re-fetch display name
                let name = if account.name.is_none() || account.name.as_ref().map_or(false, |n| n.trim().is_empty()) {
                    match oauth::get_user_info(&token_res.access_token).await {
                        Ok(user_info) => user_info.get_display_name(),
                        Err(_) => None
                    }
                } else {
                    account.name.clone()
                };
                
                account.token = new_token.clone();
                account.name = name.clone();
                upsert_account(account.email.clone(), name, new_token.clone()).map_err(AppError::Account)?;
                
                // Retry query
                let retry_result: crate::error::AppResult<(QuotaData, Option<String>)> = modules::fetch_quota(&new_token.access_token, &account.email).await;
                
                // Also handle project_id saving during retry
                if let Ok((ref _q, ref project_id)) = retry_result {
                    if project_id.is_some() && *project_id != account.token.project_id {
                        modules::logger::log_info(&format!("Detected update of project_id after retry ({}), saving...", account.email));
                        account.token.project_id = project_id.clone();
                        let _ = upsert_account(account.email.clone(), account.name.clone(), account.token.clone());
                    }
                }

                if let Err(AppError::Network(ref e)) = retry_result {
                    if let Some(s) = e.status() {
                        if s == StatusCode::FORBIDDEN {
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
            if account.disabled {
                crate::modules::logger::log_info(&format!("  - Skipping {} (Disabled)", account.email));
                return false;
            }
            if let Some(ref q) = account.quota {
                if q.is_forbidden {
                    crate::modules::logger::log_info(&format!("  - Skipping {} (Forbidden)", account.email));
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
                            crate::modules::logger::log_info(&format!("    ✅ {} Success", email));
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

    Ok(RefreshStats {
        total,
        success,
        failed,
        details,
    })
}
