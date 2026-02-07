//! Account switching logic.

use super::device::apply_profile_to_account;
use super::storage::{load_account, load_account_index, save_account, save_account_index};
use crate::modules;

/// Switch current account (Core Logic).
pub async fn switch_account(
    account_id: &str,
    integration: &(impl modules::integration::SystemIntegration + ?Sized),
) -> Result<(), String> {
    use super::crud::ACCOUNT_INDEX_LOCK;
    use crate::modules::oauth;

    let index = {
        let _lock = ACCOUNT_INDEX_LOCK.lock();
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
    // [FIX #1583] Pass account_id for proper context
    let fresh_token = oauth::ensure_fresh_token(&account.token, Some(&account.id))
        .await
        .map_err(|e| format!("Token refresh failed: {}", e))?;

    if fresh_token.access_token != account.token.access_token {
        account.token = fresh_token.clone();
        save_account(&account)?;
    }

    // 3. Ensure account has a device profile for isolation
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

    // 4. Execute platform-specific system integration
    integration.on_account_switch(&account).await?;

    // 5. Update tool internal state
    {
        let _lock = ACCOUNT_INDEX_LOCK.lock();
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
