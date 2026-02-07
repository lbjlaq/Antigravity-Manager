//! File system storage operations for accounts.

use std::fs;
use std::path::PathBuf;

use crate::models::{Account, AccountIndex};

const DATA_DIR: &str = ".antigravity_tools";
const ACCOUNTS_INDEX: &str = "accounts.json";
const ACCOUNTS_DIR: &str = "accounts";

/// Get data directory path.
pub fn get_data_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("failed_to_get_home_dir")?;
    let data_dir = home.join(DATA_DIR);

    if !data_dir.exists() {
        fs::create_dir_all(&data_dir)
            .map_err(|e| format!("failed_to_create_data_dir: {}", e))?;
    }

    Ok(data_dir)
}

/// Get accounts directory path.
pub fn get_accounts_dir() -> Result<PathBuf, String> {
    let data_dir = get_data_dir()?;
    let accounts_dir = data_dir.join(ACCOUNTS_DIR);

    if !accounts_dir.exists() {
        fs::create_dir_all(&accounts_dir)
            .map_err(|e| format!("failed_to_create_accounts_dir: {}", e))?;
    }

    Ok(accounts_dir)
}

/// Load account index.
pub fn load_account_index() -> Result<AccountIndex, String> {
    let data_dir = get_data_dir()?;
    let index_path = data_dir.join(ACCOUNTS_INDEX);

    if !index_path.exists() {
        crate::modules::logger::log_warn("Account index file not found");
        return Ok(AccountIndex::new());
    }

    let content = fs::read_to_string(&index_path)
        .map_err(|e| format!("failed_to_read_account_index: {}", e))?;

    if content.trim().is_empty() {
        crate::modules::logger::log_warn("Account index is empty, initializing new index");
        return Ok(AccountIndex::new());
    }

    let index: AccountIndex = serde_json::from_str(&content)
        .map_err(|e| format!("failed_to_parse_account_index: {}", e))?;

    crate::modules::logger::log_info(&format!(
        "Successfully loaded index with {} accounts",
        index.accounts.len()
    ));
    Ok(index)
}

/// Save account index (atomic write).
pub fn save_account_index(index: &AccountIndex) -> Result<(), String> {
    let data_dir = get_data_dir()?;
    let index_path = data_dir.join(ACCOUNTS_INDEX);
    let temp_path = data_dir.join(format!("{}.tmp", ACCOUNTS_INDEX));

    let content = serde_json::to_string_pretty(index)
        .map_err(|e| format!("failed_to_serialize_account_index: {}", e))?;

    fs::write(&temp_path, content)
        .map_err(|e| format!("failed_to_write_temp_index_file: {}", e))?;

    fs::rename(temp_path, index_path)
        .map_err(|e| format!("failed_to_replace_index_file: {}", e))
}

/// Load account data.
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

/// Save account data.
pub fn save_account(account: &Account) -> Result<(), String> {
    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account.id));

    let content = serde_json::to_string_pretty(account)
        .map_err(|e| format!("failed_to_serialize_account_data: {}", e))?;

    fs::write(&account_path, content)
        .map_err(|e| format!("failed_to_save_account_data: {}", e))
}

/// Load account data (Async).
pub async fn load_account_async(account_id: &str) -> Result<Account, String> {
    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account_id));

    if !account_path.exists() {
        return Err(format!("Account not found: {}", account_id));
    }

    let content = tokio::fs::read_to_string(&account_path)
        .await
        .map_err(|e| format!("failed_to_read_account_data: {}", e))?;

    serde_json::from_str(&content)
        .map_err(|e| format!("failed_to_parse_account_data: {}", e))
}

/// List all accounts (Async + Parallel).
pub async fn list_accounts() -> Result<Vec<Account>, String> {
    crate::modules::logger::log_info("Listing accounts (Parallel Async)...");
    let index = load_account_index()?;

    let futures: Vec<_> = index
        .accounts
        .iter()
        .map(|summary| {
            let id = summary.id.clone();
            async move {
                match load_account_async(&id).await {
                    Ok(account) => Some(account),
                    Err(e) => {
                        crate::modules::logger::log_error(&format!(
                            "Failed to load account {}: {}",
                            id, e
                        ));
                        None
                    }
                }
            }
        })
        .collect();

    let results = futures::future::join_all(futures).await;
    let accounts: Vec<Account> = results.into_iter().flatten().collect();

    Ok(accounts)
}

/// Get current account ID.
pub fn get_current_account_id() -> Result<Option<String>, String> {
    let index = load_account_index()?;
    Ok(index.current_account_id)
}

/// Get currently active account details.
pub fn get_current_account() -> Result<Option<Account>, String> {
    if let Some(id) = get_current_account_id()? {
        Ok(Some(load_account(&id)?))
    } else {
        Ok(None)
    }
}

/// Set current active account ID.
pub fn set_current_account_id(account_id: &str) -> Result<(), String> {
    let _lock = super::crud::ACCOUNT_INDEX_LOCK.lock();
    let mut index = load_account_index()?;
    index.current_account_id = Some(account_id.to_string());
    save_account_index(&index)
}
